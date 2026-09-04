//! Fixed-capacity vector backed by `arrayvec`. Usable on the RT thread; never
//! allocates after construction.

pub use arrayvec::ArrayVec as RtVec;
