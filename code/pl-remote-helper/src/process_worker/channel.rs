use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;

use pl_protocol::process_worker::{ProcessWorkerCommand, ProcessWorkerEvent};

pub(super) struct Channel {
    stream: Option<UnixStream>,
    read_open: bool,
    pending: Vec<u8>,
    offset: usize,
}

impl Channel {
    pub(super) fn read_bootstrap(
        &mut self,
        kernel: &super::kernel::Kernel,
        bytes: &mut [u8],
    ) -> io::Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            self.flush()?;
            if kernel.cancellation_signal()? {
                return Err(io::Error::from(io::ErrorKind::ConnectionAborted));
            }
            let stream = self.stream.as_mut().ok_or(io::ErrorKind::BrokenPipe)?;
            match stream.read(&mut bytes[offset..]) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(count) => {
                    offset += count;
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
            if let Err(error) = kernel.wait(self.interest(), -1)
                && error.kind() != io::ErrorKind::Interrupted
            {
                return Err(error);
            }
        }
        Ok(())
    }

    pub(super) fn stop_reading(&mut self) {
        self.read_open = false;
    }
    pub(super) fn new(stream: UnixStream) -> io::Result<Self> {
        stream.peer_addr()?;
        super::kernel::close_on_exec(stream.as_raw_fd())?;
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream: Some(stream),
            read_open: true,
            pending: Vec::new(),
            offset: 0,
        })
    }

    pub(super) fn receive(&mut self) -> io::Result<Option<ProcessWorkerCommand>> {
        if !self.read_open {
            return Ok(None);
        }
        let Some(stream) = &mut self.stream else {
            return Ok(None);
        };
        let mut byte = [0_u8];
        match stream.read(&mut byte) {
            Ok(0) => {
                self.read_open = false;
                Ok(Some(ProcessWorkerCommand::Cancel))
            }
            Ok(_) => ProcessWorkerCommand::try_from(byte[0])
                .map(Some)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(None),
            Err(error) => {
                self.read_open = false;
                Err(error)
            }
        }
    }

    pub(super) fn enqueue(&mut self, event: ProcessWorkerEvent) -> io::Result<()> {
        if self.stream.is_some() {
            self.pending
                .extend(serde_json::to_vec(&event).map_err(io::Error::other)?);
            self.pending.push(b'\n');
        }
        Ok(())
    }

    pub(super) fn flush(&mut self) -> io::Result<bool> {
        let Some(stream) = &mut self.stream else {
            return Ok(true);
        };
        while self.offset < self.pending.len() {
            match stream.write(&self.pending[self.offset..]) {
                Ok(0) => return self.disconnect(io::Error::from(io::ErrorKind::WriteZero)),
                Ok(count) => self.offset += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return self.disconnect(error),
            }
        }
        self.pending.clear();
        self.offset = 0;
        Ok(true)
    }

    fn disconnect(&mut self, error: io::Error) -> io::Result<bool> {
        self.stream = None;
        self.read_open = false;
        self.pending.clear();
        self.offset = 0;
        Err(error)
    }

    pub(super) fn interest(&self) -> Option<(RawFd, i16)> {
        let stream = self.stream.as_ref()?;
        let events = if self.read_open { libc::POLLIN } else { 0 }
            | if self.offset < self.pending.len() {
                libc::POLLOUT
            } else {
                0
            };
        (events != 0).then_some((stream.as_raw_fd(), events))
    }
}
