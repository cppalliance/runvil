//! Per-run resource ceilings: [`RunLimits`].

use std::num::{NonZero, NonZeroU32, NonZeroUsize};

/// Resource limits for one run.
///
/// The limits cover tool iterations per `models.loop` or model handle
/// `loop` call, task concurrency, Lua memory, and Lua log volume.
///
/// The defaults are built-in constants and are safe to use as they are. A
/// caller changes one by setting its field:
/// `let mut limits = RunLimits::default(); limits.max_concurrency = n;`.
/// A prompt's frontmatter `max_tool_iterations` field, when present,
/// overrides [`RunLimits::max_tool_iterations`] for that prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RunLimits {
    /// The default maximum number of tool iterations per `models.loop` or
    /// model handle `loop` call.
    ///
    /// A tool iteration is one model round of such a call; `models.infer`
    /// rounds do not count. A prompt's frontmatter `max_tool_iterations`
    /// field overrides this default for that prompt.
    pub max_tool_iterations: NonZeroU32,
    /// The run's concurrency ceiling: the most tasks the scheduler admits
    /// at once across the whole run.
    ///
    /// Each spawned task counts against its owner's limit and against every
    /// ancestor's limit, so limits nest. A fanout started inside one arm of
    /// another fanout runs within that arm's remaining share.
    pub max_concurrency: NonZeroUsize,
    /// The memory ceiling for each Lua virtual machine, in bytes.
    pub lua_memory_bytes: NonZeroUsize,
    /// The maximum number of `log` events that a prompt's Lua code can
    /// record in each Lua virtual machine.
    pub lua_log_events: NonZeroU32,
}

impl Default for RunLimits {
    /// Returns limits with the default values: 24 tool iterations per
    /// `models.loop` or model handle `loop` call, 8 concurrent tasks,
    /// 64 MiB of Lua memory, and 1024 Lua log events.
    fn default() -> RunLimits {
        RunLimits {
            max_tool_iterations: const { NonZero::new(24).unwrap() },
            max_concurrency: const { NonZero::new(8).unwrap() },
            lua_memory_bytes: const { NonZero::new(64 * 1024 * 1024).unwrap() },
            lua_log_events: const { NonZero::new(1024).unwrap() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_limits_pins_all_four_defaults() {
        let defaults = RunLimits::default();
        assert_eq!(defaults.max_tool_iterations.get(), 24);
        assert_eq!(defaults.max_concurrency.get(), 8);
        assert_eq!(defaults.lua_memory_bytes.get(), 64 * 1024 * 1024);
        assert_eq!(defaults.lua_log_events.get(), 1024);
    }

    #[test]
    fn limits_compare_and_hash_by_value() {
        let tighter = RunLimits {
            max_tool_iterations: const { NonZero::new(8).unwrap() },
            ..RunLimits::default()
        };
        assert_ne!(RunLimits::default(), tighter);
        let distinct: std::collections::HashSet<RunLimits> =
            [RunLimits::default(), RunLimits::default(), tighter]
                .into_iter()
                .collect();
        assert_eq!(distinct.len(), 2, "equal limits hash alike");
    }
}
