import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronDown } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { TimelineEntry } from "../state/selectors";

type FollowMode = "following" | "paused";

type ConversationTimelineProps = {
  sessionId: string | null;
  entries: TimelineEntry[];
  isBusy: boolean;
  emptyState: ReactNode;
  scrollToBottomLabel: string;
  renderEntry: (entry: TimelineEntry) => ReactNode;
};

const SCROLL_THRESHOLD = 40;

export function ConversationTimeline({
  sessionId,
  entries,
  isBusy,
  emptyState,
  scrollToBottomLabel,
  renderEntry,
}: ConversationTimelineProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const followModeRef = useRef<FollowMode>("following");
  const programmaticScrollRef = useRef(false);
  const userInteractingRef = useRef(false);
  const userInteractingTimerRef = useRef<number | null>(null);
  const didRenderEntriesRef = useRef(false);
  const prevBusyRef = useRef(isBusy);
  const [showScrollButton, setShowScrollButton] = useState(false);

  const virtualizer = useVirtualizer<HTMLDivElement, HTMLDivElement>({
    count: entries.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) => estimateEntrySize(entries[index]),
    getItemKey: (index) => entries[index]?.key ?? index,
    overscan: 8,
    anchorTo: "end",
    followOnAppend: "auto",
    scrollEndThreshold: SCROLL_THRESHOLD,
    useAnimationFrameWithResizeObserver: true,
    onChange: (instance) => {
      const atBottom = instance.isAtEnd(SCROLL_THRESHOLD);
      if (atBottom) {
        followModeRef.current = "following";
        userInteractingRef.current = false;
      }
      setShowScrollButton(!atBottom || followModeRef.current === "paused");
    },
  });

  useEffect(() => {
    followModeRef.current = "following";
    userInteractingRef.current = false;
    didRenderEntriesRef.current = false;
    setShowScrollButton(false);
    window.requestAnimationFrame(() => scrollToLatest("force"));
  }, [sessionId]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const markUserInteraction = () => {
      userInteractingRef.current = true;
      if (userInteractingTimerRef.current !== null) {
        window.clearTimeout(userInteractingTimerRef.current);
      }
      userInteractingTimerRef.current = window.setTimeout(() => {
        userInteractingRef.current = false;
        userInteractingTimerRef.current = null;
      }, 220);
    };

    const handleScroll = () => {
      const atBottom = isNearBottom();
      if (programmaticScrollRef.current) {
        setShowScrollButton(false);
        return;
      }
      if (atBottom) {
        followModeRef.current = "following";
        userInteractingRef.current = false;
      } else if (userInteractingRef.current) {
        followModeRef.current = "paused";
      }
      setShowScrollButton(!atBottom || followModeRef.current === "paused");
    };

    const handleUserScrollIntent = () => {
      markUserInteraction();
      window.requestAnimationFrame(handleScroll);
    };

    el.addEventListener("wheel", handleUserScrollIntent, { passive: true });
    el.addEventListener("touchmove", handleUserScrollIntent, { passive: true });
    el.addEventListener("pointerdown", handleUserScrollIntent);
    el.addEventListener("keydown", handleUserScrollIntent);
    el.addEventListener("scroll", handleScroll, { passive: true });
    return () => {
      el.removeEventListener("wheel", handleUserScrollIntent);
      el.removeEventListener("touchmove", handleUserScrollIntent);
      el.removeEventListener("pointerdown", handleUserScrollIntent);
      el.removeEventListener("keydown", handleUserScrollIntent);
      el.removeEventListener("scroll", handleScroll);
      if (userInteractingTimerRef.current !== null) {
        window.clearTimeout(userInteractingTimerRef.current);
      }
    };
  }, [virtualizer]);

  useLayoutEffect(() => {
    if (entries.length === 0) {
      didRenderEntriesRef.current = false;
      setShowScrollButton(false);
      return;
    }
    if (!didRenderEntriesRef.current) {
      didRenderEntriesRef.current = true;
      scrollToLatest("force");
      return;
    }
    if (followModeRef.current === "following") {
      scrollToLatest("preserve");
    } else {
      setShowScrollButton(true);
    }
  }, [entries, virtualizer]);

  useEffect(() => {
    if (isBusy && !prevBusyRef.current) {
      followModeRef.current = "following";
      scrollToLatest("force");
    }
    prevBusyRef.current = isBusy;
  }, [isBusy]);

  function isNearBottom(): boolean {
    const el = scrollRef.current;
    if (!el) return true;
    return el.scrollHeight - el.scrollTop - el.clientHeight <= SCROLL_THRESHOLD;
  }

  function scrollToLatest(mode: "preserve" | "force") {
    if (entries.length === 0) return;
    if (mode === "preserve" && followModeRef.current !== "following") {
      setShowScrollButton(true);
      return;
    }
    if (mode === "force") {
      followModeRef.current = "following";
    }
    programmaticScrollRef.current = true;

    const scroll = () => {
      virtualizer.scrollToEnd({ behavior: "auto" });
      const el = scrollRef.current;
      if (el) {
        el.scrollTop = el.scrollHeight;
      }
      userInteractingRef.current = false;
      setShowScrollButton(false);
    };

    scroll();
    window.requestAnimationFrame(() => {
      if (mode === "preserve" && followModeRef.current !== "following") {
        programmaticScrollRef.current = false;
        return;
      }
      scroll();
      window.requestAnimationFrame(() => {
        if (mode === "preserve" && followModeRef.current !== "following") {
          programmaticScrollRef.current = false;
          return;
        }
        scroll();
        window.setTimeout(() => {
          if (mode === "preserve" && followModeRef.current !== "following") {
            programmaticScrollRef.current = false;
            return;
          }
          scroll();
          programmaticScrollRef.current = false;
        }, 80);
      });
    });
  }

  function scrollToBottom() {
    followModeRef.current = "following";
    scrollToLatest("force");
  }

  const virtualItems = virtualizer.getVirtualItems();

  return (
    <div className="message-stream" ref={scrollRef}>
      {entries.length === 0 ? (
        emptyState
      ) : (
        <div className="conversation-timeline">
          <div className="timeline-virtual-spacer" style={{ height: `${virtualizer.getTotalSize()}px` }}>
            {virtualItems.map((virtualItem) => {
              const entry = entries[virtualItem.index];
              if (!entry) return null;
              return (
                <div
                  key={virtualItem.key}
                  className="timeline-virtual-row"
                  data-index={virtualItem.index}
                  ref={virtualizer.measureElement}
                  style={{ transform: `translateY(${virtualItem.start}px)` }}
                >
                  {renderEntry(entry)}
                </div>
              );
            })}
          </div>
        </div>
      )}
      {showScrollButton && (
        <button
          className="scroll-to-bottom"
          onClick={scrollToBottom}
          title={scrollToBottomLabel}
          aria-label={scrollToBottomLabel}
        >
          <ChevronDown size={18} />
        </button>
      )}
    </div>
  );
}

function estimateEntrySize(entry: TimelineEntry | undefined): number {
  switch (entry?.kind) {
    case "message":
      return entry.role === "user" ? 96 : 140;
    case "plan":
      return 220;
    case "thought":
      return 74;
    case "tool":
      return 86;
    case "toolGroup":
      return 96;
    case "agent":
      return 104;
    case "trace":
      return 72;
    case undefined:
      return 96;
  }
}
