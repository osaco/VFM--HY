//! Local-only link shim for six C math symbols missing from this host's CRT stubs.
//! Rust 1.98's rust-lld on windows-msvc cannot resolve cbrt/log2/fma/rint when no
//! Windows SDK Lib/ucrt.lib is installed. These implementations use only IEEE-754
//! bit inspection and + - * / so they never recurse into another C math symbol.
//! This crate is NOT production code and is not linked into shipping builds.

const LN2_2: f64 = 1.4426950408889634;

#[no_mangle]
pub extern "C" fn fma(a: f64, b: f64, c: f64) -> f64 {
    a * b + c
}

#[no_mangle]
pub extern "C" fn fmaf(a: f32, b: f32, c: f32) -> f32 {
    a * b + c
}

#[inline]
fn rint_f64(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    if x >= 0.0 {
        ((x + 0.5) as u128) as f64
    } else {
        -((((-x) + 0.5) as u128) as f64)
    }
}

#[no_mangle]
pub extern "C" fn rint(x: f64) -> f64 {
    rint_f64(x)
}

#[no_mangle]
pub extern "C" fn rintf(x: f32) -> f32 {
    rint_f64(x as f64) as f32
}

#[inline]
fn natural_log(m: f64) -> f64 {
    // atanh series: ln(m) = 2*(y + y^3/3 + y^5/5 + ...), y=(m-1)/(m+1), |y|<=1/3.
    let y = (m - 1.0) / (m + 1.0);
    let y2 = y * y;
    let mut term = y;
    let mut sum = y;
    for k in 1..12 {
        term *= y2;
        sum += term / (2 * k + 1) as f64;
    }
    2.0 * sum
}

#[no_mangle]
pub extern "C" fn log2(x: f64) -> f64 {
    if x <= 0.0 || !x.is_finite() {
        return if x == 0.0 { f64::NEG_INFINITY } else if x > 0.0 { x } else { f64::NAN };
    }
    let mut bits = x.to_bits();
    let mut exp = ((bits >> 52) & 0x7ff) as i32;
    if exp == 0x7ff {
        return if (bits & ((1u64 << 52) - 1)) == 0 { f64::INFINITY } else { x };
    }
    let mut adjust = 0.0;
    if exp == 0 {
        // Subnormal: scale into the normal range and compensate the exponent.
        let scaled = x * 4503599627370496.0; // 2^52
        bits = scaled.to_bits();
        exp = ((bits >> 52) & 0x7ff) as i32;
        adjust = -52.0;
    }
    let mantissa = f64::from_bits((bits & ((1u64 << 52) - 1)) | 0x3ff0_0000_0000_0000);
    (exp - 1023) as f64 + adjust + natural_log(mantissa) * LN2_2
}

#[no_mangle]
pub extern "C" fn log2f(x: f32) -> f32 {
    log2(x as f64) as f32
}

#[no_mangle]
pub extern "C" fn cbrt(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let negative = x < 0.0;
    let a = x.abs();
    let mut y = if a > 1.0 { a / 3.0 + 0.25 } else { 0.5 * a + 0.25 };
    for _ in 0..40 {
        let y2 = y * y;
        if y2 == 0.0 || !y2.is_finite() {
            break;
        }
        let next = (2.0 * y + a / y2) / 3.0;
        if (next - y).abs() <= (y.abs() * 1e-15) {
            y = next;
            break;
        }
        y = next;
    }
    if negative { -y } else { y }
}

#[no_mangle]
pub extern "C" fn cbrtf(x: f32) -> f32 {
    cbrt(x as f64) as f32
}

const LN2: f64 = 0.6931471805599453;
const LOG2E: f64 = 1.4426950408889634;

#[no_mangle]
pub extern "C" fn exp2(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let n = rint_f64(x);
    if n > 1023.0 {
        return f64::INFINITY;
    }
    if n < -1022.0 {
        return 0.0;
    }
    let f = x - n;
    let z = f * LN2;
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..18 {
        term *= z / k as f64;
        sum += term;
    }
    sum * f64::from_bits((((n as i64) + 1023) as u64) << 52)
}

#[no_mangle]
pub extern "C" fn exp2f(x: f32) -> f32 {
    exp2(x as f64) as f32
}

#[no_mangle]
pub extern "C" fn exp(x: f64) -> f64 {
    exp2(x * LOG2E)
}

#[no_mangle]
pub extern "C" fn expf(x: f32) -> f32 {
    exp2(x as f64 * LOG2E) as f32
}

