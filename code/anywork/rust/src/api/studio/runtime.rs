use anyhow::{Context, Result};
use pl_studio_runtime::{
    RemoteHelperSource, StudioRuntime, StudioRuntimeOptions, StudioRuntimeStateKind,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{OnceCell, SetError};
use tokio_util::sync::CancellationToken;

use super::subscription::BridgeTaskRegistry;
use super::types::BridgeError;

static BRIDGE: OnceCell<BridgeRuntime> = OnceCell::const_new();

/// 单向退出终止闩：一旦应用退出决定不安装 runtime，本进程余下生命周期内拒绝任何迟到安装。
static EXIT_LATCH: AtomicBool = AtomicBool::new(false);

/// 迟到安装被拒的回执：初始化确曾启动并创建过资源，退出不能伪报 `NotStarted`。
static LATE_INSTALL_REFUSED: AtomicBool = AtomicBool::new(false);

/// 本次退出唯一的绝对清理期限；由首个退出入口固定，重复退出不延长。
static EXIT_DEADLINE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

/// 退出清理预算上限，与 runtime 的 `EXIT_CLEANUP_BUDGET` 一致，但由 bridge 侧独立钳制。
pub(crate) const BRIDGE_EXIT_CLEANUP_BUDGET: Duration = Duration::from_secs(28);

/// 串行化安装与退出决策，使「安装」与「退出判定为 NotStarted」原子决定发布。
static LIFECYCLE_GATE: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// 单一生命周期 publication/封闭 linearization 事实源。
///
/// 「封闭退出闩并观察已发布 runtime」与「二次核对闩并发布 runtime」必须原子地互相排斥，否则
/// 首 close 与已在初始化中的安装会在「检查闩 → `BRIDGE.set`」之间交错，令 runtime 在闩已封闭后
/// 仍被发布。该锁只在无 await、无 IO、无外部调用的极短临界区内持有——绝不在初始化或任何 await
/// 期间持有——因此即使被竞争也是有界等待，不会拖住 native 退出期限。
static PUBLICATION_GATE: OnceLock<Mutex<()>> = OnceLock::new();

/// 迟到安装被拒绝后，由统一 bridge 生命周期强持有的 runtime owner。
///
/// `BRIDGE` 仍是唯一的「已发布 runtime」事实源：关闭胜出后绝不再发布。本槽位只承载被拒绝安装
/// 已经创建的 owner（实例锁 / writer / DB / service job），保证报告为 `Degraded` 时不因安装任务
/// 的局部变量离开作用域而提前释放实例锁或关闭仍在写的数据库；它一直保留到真实 Clean ACK 或进程
/// 终止。最终退出入口经 `arm_exit_latch` 观察它，并用同一 owned 关闭编排（`ShutdownRun`）收束，
/// 不新建第二套 cleanup，也不把它发布为 active/ready。
static RETAINED_REFUSED_RUNTIME: OnceLock<BridgeRuntime> = OnceLock::new();

/// 把被拒绝发布的 built runtime 移入进程级强持有槽并返回其引用。
///
/// 只有一个安装任务能失去 publication（安装经 `lifecycle_gate` 串行化），因此正常不会发生覆盖；
/// 万一有并发安装，已有 owner 也会被保留：这里绝不允许 drop 任何一个已创建 owner。
fn retain_refused_runtime(built: BridgeRuntime) -> &'static BridgeRuntime {
    if let Err(built) = RETAINED_REFUSED_RUNTIME.set(built) {
        tracing::error!(
            pid = std::process::id(),
            "a second refused Studio runtime owner was retained; keeping both owners alive"
        );
        // Defensive, unreachable in practice: keep the redundant owner alive to process exit rather
        // than dropping its instance lock / writer / DB owner.
        let _ = Box::leak(Box::new(built));
    }
    RETAINED_REFUSED_RUNTIME
        .get()
        .expect("refused Studio runtime owner was just retained")
}

fn publication_gate() -> &'static Mutex<()> {
    PUBLICATION_GATE.get_or_init(|| Mutex::new(()))
}

pub(crate) fn lifecycle_gate() -> &'static tokio::sync::Mutex<()> {
    LIFECYCLE_GATE.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub(crate) fn exit_latch_armed() -> bool {
    EXIT_LATCH.load(Ordering::SeqCst)
}

/// 封闭单向退出闩，并与封闭决定原子地观察「已发布 runtime 或被拒绝保留的 runtime owner」。
///
/// 返回 `Some` 表示本次退出已有一个 bridge 生命周期 owner 需要收束：既可能是安装先胜出、已发布
/// 的 runtime，也可能是迟到安装被拒绝后由 `RETAINED_REFUSED_RUNTIME` 强持有的 owner；两者都必须
/// 纳入同一 owned 关闭编排。返回 `None` 表示关闭胜出且当时没有任何已创建 owner：此后任何安装都
/// 不会再被发布（发布与封闭在 `PUBLICATION_GATE` 上线性化），因此 `None` 是稳定的。
pub(crate) fn arm_exit_latch() -> Option<&'static BridgeRuntime> {
    let _guard = publication_gate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    EXIT_LATCH.store(true, Ordering::SeqCst);
    BRIDGE.get().or_else(|| RETAINED_REFUSED_RUNTIME.get())
}

pub(crate) fn late_install_refused() -> bool {
    LATE_INSTALL_REFUSED.load(Ordering::SeqCst)
}

/// 固定本次退出的唯一绝对期限；首次调用生效，后续调用只取更早者，永不延长。
pub(crate) fn fix_exit_deadline(remaining_ms: u32) -> Instant {
    let requested = Duration::from_millis(u64::from(remaining_ms)).min(BRIDGE_EXIT_CLEANUP_BUDGET);
    let candidate = Instant::now() + requested;
    let slot = EXIT_DEADLINE.get_or_init(|| Mutex::new(None));
    let mut guard = match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let fixed = guard.map_or(candidate, |existing| existing.min(candidate));
    *guard = Some(fixed);
    fixed
}

/// 共享期限下的剩余时间；已耗尽返回零。
pub(crate) fn remaining_until(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

/// 本次退出已固定的首个绝对期限的剩余毫秒；若尚未固定，则按 28s 首次预算固定后返回其剩余。
///
/// 迟到拒绝的安装分支必须复用同一首个期限，不能从较晚时刻再起一个新的完整预算，重复关闭也不
/// 延长。`fix_exit_deadline` 只取更早者，因此这里只会复用或缩短既有期限。
fn remaining_exit_budget_ms() -> u32 {
    let deadline = fix_exit_deadline(BRIDGE_EXIT_CLEANUP_BUDGET.as_millis() as u32);
    remaining_until(deadline).as_millis().min(u32::MAX as u128) as u32
}

pub(crate) fn current_bridge() -> Option<&'static BridgeRuntime> {
    BRIDGE.get()
}

pub(crate) struct BridgeRuntime {
    pub(crate) studio: StudioRuntime,
    pub(crate) subscriptions: BridgeTaskRegistry,
    pub(crate) shutdown: CancellationToken,
}

impl BridgeRuntime {
    async fn new() -> Result<Self> {
        Ok(Self {
            studio: StudioRuntime::initialize_with_observer(
                desktop_runtime_options()?,
                std::sync::Arc::new(super::handlers::lifecycle::publish_startup_stage),
            )
            .await?,
            subscriptions: BridgeTaskRegistry::new(),
            shutdown: CancellationToken::new(),
        })
    }
}

/// Desktop bridge always runs from the packaged bundle, so its helper bytes come
/// from the resources installed beside the application executable.
fn desktop_runtime_options() -> Result<StudioRuntimeOptions> {
    let executable = std::env::current_exe()
        .context("locate the application executable to resolve bundled remote helper resources")?;
    let bundle_root = executable
        .parent()
        .context("application executable has no parent directory")?;
    Ok(
        StudioRuntimeOptions::desktop().with_remote_helper_source(RemoteHelperSource::Bundled(
            bundle_root.join("data").join("remote-helper"),
        )),
    )
}

/// 构造并安装 Bridge runtime；只能由显式启动命令调用。
pub(crate) async fn install_bridge_runtime() -> Result<&'static BridgeRuntime> {
    let _gate = lifecycle_gate().lock().await;
    if exit_latch_armed() {
        anyhow::bail!("application exit has begun; refusing to install the Studio runtime");
    }
    if let Some(bridge) = BRIDGE.get() {
        return Ok(bridge);
    }
    // 初始化期间退出闩可能被封闭；发布经 `publish_bridge_runtime` 与封闭在同一 linearization
    // 事实源上原子决定，关闭胜出后绝不再发布，安装先胜出的 runtime 必被 begin/final 观察。
    let built = BridgeRuntime::new().await?;
    match publish_bridge_runtime(built) {
        Ok(bridge) => Ok(bridge),
        Err(built) => {
            // 关闭胜出：本任务把这次初始化创建的资源 owner 移入统一生命周期强持有槽，并用与退出
            // 完全相同的 owned 编排（同一首个期限剩余预算、同一 `ShutdownRun`）收束它。`Degraded`
            // 报告不会释放实例锁 / 关闭数据库：owner 仍被强持有到真实 Clean ACK 或进程终止；迟到
            // 的 `shutdown_runtime` 会经 `arm_exit_latch` 观察同一 owner，绝不伪报 `NotStarted`。
            LATE_INSTALL_REFUSED.store(true, Ordering::SeqCst);
            let retained = retain_refused_runtime(*built);
            let report = retained
                .studio
                .shutdown_runtime_for_exit(remaining_exit_budget_ms(), Vec::new())
                .await;
            if !report.is_clean() {
                tracing::error!(
                    pid = std::process::id(),
                    outcome = ?report.outcome,
                    issue_count = report.issues.len(),
                    "late Studio runtime install was refused; its shared-orchestration cleanup did not reach Clean and its owners are retained until process exit"
                );
            }
            anyhow::bail!("application exit has begun; refusing to publish the Studio runtime");
        }
    }
}

/// Publishes the built runtime unless exit already closed the latch.
///
/// `Ok` means the install won: the runtime is published and any later `arm_exit_latch` observes it.
/// `Err` returns the built runtime boxed so its owner can reclaim it without the `Err`-variant
/// carrying the whole runtime by value; the caller's install did not win.
fn publish_bridge_runtime(
    built: BridgeRuntime,
) -> std::result::Result<&'static BridgeRuntime, Box<BridgeRuntime>> {
    let _guard = publication_gate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if EXIT_LATCH.load(Ordering::SeqCst) {
        return Err(Box::new(built));
    }
    match BRIDGE.set(built) {
        Ok(()) => Ok(BRIDGE
            .get()
            .expect("bridge runtime was just published under the publication gate")),
        // `tokio::sync::OnceCell::set` 以 `SetError` 枚举归还被拒绝的值；取回并交回调用方保留
        // owner（装箱避免 `Err` 变体承载整个 runtime），不新增任何 From/错误转义，也不丢弃 built。
        Err(SetError::AlreadyInitializedError(built) | SetError::InitializingError(built)) => {
            Err(Box::new(built))
        }
    }
}

pub(crate) fn installed_bridge() -> Result<&'static BridgeRuntime, BridgeError> {
    BRIDGE.get().ok_or_else(BridgeError::not_initialized)
}

pub(crate) async fn active_bridge() -> Result<&'static BridgeRuntime, BridgeError> {
    // The exit latch is closed synchronously at the first exit entry, before any await. Refuse new
    // mutations from that instant even if the runtime state has not transitioned yet, so a
    // concurrent exit can never race a late install or a new bridge command.
    if exit_latch_armed() {
        return Err(BridgeError::runtime_stopped());
    }
    let bridge = installed_bridge()?;
    match bridge.studio.runtime_snapshot().await?.state.kind() {
        StudioRuntimeStateKind::Ready => Ok(bridge),
        StudioRuntimeStateKind::Uninitialized | StudioRuntimeStateKind::Initializing => {
            Err(BridgeError::not_initialized())
        }
        StudioRuntimeStateKind::ShuttingDown
        | StudioRuntimeStateKind::Stopped
        | StudioRuntimeStateKind::Failed => Err(BridgeError::runtime_stopped()),
    }
}
