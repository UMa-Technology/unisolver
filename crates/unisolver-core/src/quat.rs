//! Quaternion components from a numeris `Quaternion` without relying on its field
//! layout: rebuild the rotation matrix from three vector rotations, then apply
//! Shepperd's method. Convention: camera = q * icrs.
use numeris::{Quaternion, Vector3};

pub(crate) fn quat_to_wxyz(q: &Quaternion<f32>) -> [f32; 4] {
    let c0 = *q * Vector3::from_array([1.0f32, 0.0, 0.0]);
    let c1 = *q * Vector3::from_array([0.0f32, 1.0, 0.0]);
    let c2 = *q * Vector3::from_array([0.0f32, 0.0, 1.0]);
    // Column i of R is R·e_i; indexed R[r][c]
    let r = [
        [c0[0], c1[0], c2[0]],
        [c0[1], c1[1], c2[1]],
        [c0[2], c1[2], c2[2]],
    ];
    let tr = r[0][0] + r[1][1] + r[2][2];
    let (w, x, y, z);
    if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        w = 0.25 * s;
        x = (r[2][1] - r[1][2]) / s;
        y = (r[0][2] - r[2][0]) / s;
        z = (r[1][0] - r[0][1]) / s;
    } else if r[0][0] > r[1][1] && r[0][0] > r[2][2] {
        let s = (1.0 + r[0][0] - r[1][1] - r[2][2]).sqrt() * 2.0;
        w = (r[2][1] - r[1][2]) / s;
        x = 0.25 * s;
        y = (r[0][1] + r[1][0]) / s;
        z = (r[0][2] + r[2][0]) / s;
    } else if r[1][1] > r[2][2] {
        let s = (1.0 + r[1][1] - r[0][0] - r[2][2]).sqrt() * 2.0;
        w = (r[0][2] - r[2][0]) / s;
        x = (r[0][1] + r[1][0]) / s;
        y = 0.25 * s;
        z = (r[1][2] + r[2][1]) / s;
    } else {
        let s = (1.0 + r[2][2] - r[0][0] - r[1][1]).sqrt() * 2.0;
        w = (r[1][0] - r[0][1]) / s;
        x = (r[0][2] + r[2][0]) / s;
        y = (r[1][2] + r[2][1]) / s;
        z = 0.25 * s;
    }
    // Canonicalize to the w >= 0 hemisphere so the output is stable
    if w < 0.0 {
        [-w, -x, -y, -z]
    } else {
        [w, x, y, z]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_via_new() {
        // Unit quaternion (normalized (0.82, 0.1, -0.3, 0.47)): for a non-unit q,
        // q·v·q* carries a |q|² scale
        let q = Quaternion::new(0.822761f32, 0.100337, -0.30101, 0.471582);
        let a = quat_to_wxyz(&q);
        let q2 = Quaternion::new(a[0], a[1], a[2], a[3]);
        let v = Vector3::from_array([0.3f32, -0.7, 0.64]);
        let (u, w) = (q * v, q2 * v);
        for i in 0..3 {
            assert!((u[i] - w[i]).abs() < 1e-4);
        }
    }
}
