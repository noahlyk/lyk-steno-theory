//! Counts as floats, for averages and shares. Every count this program keeps (words, strokes,
//! keys, how often a word is said in the corpus) is far below 2^52, where an `f64` holds every
//! integer exactly, so converting one loses nothing: this is the one place that says so.

pub trait ToF64 {
    fn to_f64(self) -> f64;
}

impl ToF64 for u64 {
    #[allow(clippy::cast_precision_loss)]
    fn to_f64(self) -> f64 {
        self as f64
    }
}

impl ToF64 for usize {
    #[allow(clippy::cast_precision_loss)]
    fn to_f64(self) -> f64 {
        self as f64
    }
}

impl ToF64 for u32 {
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}
