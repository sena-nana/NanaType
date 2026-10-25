//! The process's GPU scheduling priority. The UI thread draws every window
//! and its Spout sender, so the whole process is raised.

/// Raises (or restores) this process's GPU priority. Returns whether `high`
/// is in effect: a High request one of the Windows controls refused is
/// rolled back to Normal, and other platforms are always Normal.
pub fn apply_gpu(high: bool) -> bool {
    #[cfg(windows)]
    {
        windows_impl::apply(high)
    }
    #[cfg(not(windows))]
    {
        let _ = high;
        false
    }
}

#[cfg(windows)]
mod windows_impl {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Threading::{
        GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
        ProcessPowerThrottling, SetProcessInformation,
    };

    pub(super) fn apply(high: bool) -> bool {
        // SAFETY: the pseudo-handle of this process needs no closing.
        let handle = unsafe { GetCurrentProcess() };
        let power_applied = apply_power_throttling(handle, high);
        let scheduling_applied = apply_scheduling_class(handle, high);
        let applied = power_applied && scheduling_applied;
        if !applied {
            // Do not leave a partially changed process behind when one of the
            // Windows controls refused the request. This is best effort: the
            // result is still Normal if the rollback itself is refused.
            let _ = apply_power_throttling(handle, false);
            let _ = apply_scheduling_class(handle, false);
        }
        high && applied
    }

    fn apply_power_throttling(handle: HANDLE, high: bool) -> bool {
        let state = power_throttling_state(high);
        // SAFETY: `state` is a complete PROCESS_POWER_THROTTLING_STATE of the
        // size passed, alive for the call.
        unsafe {
            SetProcessInformation(
                handle,
                ProcessPowerThrottling,
                (&raw const state).cast(),
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        }
        .is_ok()
    }

    pub(super) fn power_throttling_state(high: bool) -> PROCESS_POWER_THROTTLING_STATE {
        // ControlMask names the policies this call takes over and StateMask turns
        // them on. High takes execution speed over and turns throttling off;
        // Normal takes nothing over, which hands the decision back to Windows.
        // Setting both bits would opt the whole process into EcoQoS.
        PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: if high {
                PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            } else {
                0
            },
            StateMask: 0,
        }
    }

    fn apply_scheduling_class(handle: HANDLE, high: bool) -> bool {
        #[link(name = "gdi32")]
        unsafe extern "system" {
            fn D3DKMTSetProcessSchedulingPriorityClass(process: HANDLE, priority_class: i32)
            -> i32;
        }

        // SAFETY: a documented gdi32 export taking a process handle and a
        // D3DKMT_SCHEDULINGPRIORITYCLASS value; it returns an NTSTATUS.
        unsafe { D3DKMTSetProcessSchedulingPriorityClass(handle, scheduling_class(high)) == 0 }
    }

    /// D3DKMT_SCHEDULINGPRIORITYCLASS: Normal is 2, High is 4.
    pub(super) fn scheduling_class(high: bool) -> i32 {
        if high { 4 } else { 2 }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::windows_impl::*;

    #[test]
    fn power_throttling_mapping_matches_priority() {
        // Normal leaves the policy to the system instead of forcing EcoQoS.
        let normal = power_throttling_state(false);
        assert_eq!((normal.ControlMask, normal.StateMask), (0, 0));
        // High explicitly disables execution-speed throttling.
        let high = power_throttling_state(true);
        assert_eq!((high.ControlMask, high.StateMask), (1, 0));
    }

    #[test]
    fn scheduling_mapping_matches_priority() {
        assert_eq!(scheduling_class(false), 2);
        assert_eq!(scheduling_class(true), 4);
    }

    #[test]
    fn normal_is_never_reported_as_raised() {
        assert!(!super::apply_gpu(false));
    }
}

#[cfg(all(test, not(windows)))]
mod non_windows_tests {
    #[test]
    fn unsupported_target_is_never_raised() {
        assert!(!super::apply_gpu(true));
    }
}
