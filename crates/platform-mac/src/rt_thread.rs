//! Mach thread-policy helpers for promoting the RT audio thread.

pub fn promote_to_realtime(buffer_frames: u32, sample_rate: u32) {
    use mach2::mach_types::thread_act_t;
    use mach2::thread_policy::{
        thread_policy_set, thread_time_constraint_policy_data_t, THREAD_TIME_CONSTRAINT_POLICY,
    };

    if buffer_frames == 0 || sample_rate == 0 {
        return;
    }
    let mach_s = super::host_time::mach_sec_per_tick();
    if mach_s <= 0.0 {
        return;
    }
    let period_sec = buffer_frames as f64 / sample_rate as f64;
    let period_ticks = (period_sec / mach_s) as u32;
    let computation_ticks = (period_ticks as f64 * 0.5) as u32;
    let constraint_ticks = (period_ticks as f64 * 0.85) as u32;
    let mut policy = thread_time_constraint_policy_data_t {
        period: period_ticks,
        computation: computation_ticks,
        constraint: constraint_ticks,
        preemptible: 0,
    };
    unsafe {
        let thread: thread_act_t = mach2::mach_init::mach_thread_self();
        thread_policy_set(
            thread,
            THREAD_TIME_CONSTRAINT_POLICY,
            (&mut policy as *mut _) as *mut _,
            (std::mem::size_of::<thread_time_constraint_policy_data_t>()
                / std::mem::size_of::<u32>()) as u32,
        );
    }
}
