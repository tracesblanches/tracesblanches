//! Small dense linear algebra: LU solve with partial pivoting (row-major matrices).

/// Solves `a · x = b` for `m` right-hand sides. `a` is `n × n` row-major, `b` is `n × m`
/// row-major; returns `x` (`n × m`), or `None` when `a` is singular to working precision.
pub fn solve(mut a: Vec<f64>, n: usize, mut b: Vec<f64>, m: usize) -> Option<Vec<f64>> {
    debug_assert_eq!(a.len(), n * n);
    debug_assert_eq!(b.len(), n * m);
    let scale = a.iter().fold(0.0_f64, |s, v| s.max(v.abs()));
    if scale == 0.0 || !scale.is_finite() {
        return None;
    }
    let tiny = scale * 1e-14;
    for k in 0..n {
        let (mut piv, mut best) = (k, a[k * n + k].abs());
        for r in k + 1..n {
            let v = a[r * n + k].abs();
            if v > best {
                (piv, best) = (r, v);
            }
        }
        if best <= tiny {
            return None;
        }
        if piv != k {
            for c in 0..n {
                a.swap(k * n + c, piv * n + c);
            }
            for c in 0..m {
                b.swap(k * m + c, piv * m + c);
            }
        }
        let d = a[k * n + k];
        for r in k + 1..n {
            let f = a[r * n + k] / d;
            if f == 0.0 {
                continue;
            }
            a[r * n + k] = 0.0;
            for c in k + 1..n {
                a[r * n + c] -= f * a[k * n + c];
            }
            for c in 0..m {
                b[r * m + c] -= f * b[k * m + c];
            }
        }
    }
    let mut x = vec![0.0; n * m];
    for k in (0..n).rev() {
        for c in 0..m {
            let mut s = b[k * m + c];
            for j in k + 1..n {
                s -= a[k * n + j] * x[j * m + c];
            }
            x[k * m + c] = s / a[k * n + k];
        }
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Least squares `min |A x - b|` through the normal equations (`A` is `rows × n`
/// row-major, one right-hand side per column of `b`, `rows × m`).
pub fn least_squares(a: &[f64], rows: usize, n: usize, b: &[f64], m: usize) -> Option<Vec<f64>> {
    let mut ata = vec![0.0; n * n];
    let mut atb = vec![0.0; n * m];
    for r in 0..rows {
        let row = &a[r * n..(r + 1) * n];
        for i in 0..n {
            for j in 0..n {
                ata[i * n + j] += row[i] * row[j];
            }
            for c in 0..m {
                atb[i * m + c] += row[i] * b[r * m + c];
            }
        }
    }
    solve(ata, n, atb, m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_a_pivoting_system() {
        // First pivot is zero: needs a row swap.
        let a = vec![0.0, 2.0, 1.0, 1.0, 1.0, 0.0, 3.0, 0.0, 1.0];
        let x_true = [1.0, -2.0, 3.0];
        let b: Vec<f64> = (0..3)
            .map(|r| (0..3).map(|c| a[r * 3 + c] * x_true[c]).sum())
            .collect();
        let x = solve(a, 3, b, 1).unwrap();
        for (u, v) in x.iter().zip(x_true) {
            assert!((u - v).abs() < 1e-12);
        }
    }

    #[test]
    fn rejects_a_singular_system() {
        let a = vec![1.0, 2.0, 2.0, 4.0];
        assert!(solve(a, 2, vec![1.0, 2.0], 1).is_none());
    }

    #[test]
    fn fits_a_line_by_least_squares() {
        // y = 2 + 3 x, exact.
        let xs = [0.0, 1.0, 2.0, 5.0];
        let a: Vec<f64> = xs.iter().flat_map(|&x| [1.0, x]).collect();
        let b: Vec<f64> = xs.iter().map(|&x| 2.0 + 3.0 * x).collect();
        let p = least_squares(&a, 4, 2, &b, 1).unwrap();
        assert!((p[0] - 2.0).abs() < 1e-10 && (p[1] - 3.0).abs() < 1e-10);
    }
}
