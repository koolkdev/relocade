//! 500-bit tangent references after exact P4 pi66 reduction.
//! Arbitrarily tiny positive tangents use x < tan(x) < x + half an ulp.

use crate::support::x87::reference::Bounds;
pub(super) const VALUES: &[((u64, u16), Bounds)] = &[
    (
        (0x0000000000000000, 0x0000),
        Bounds {
            lower: (0, 0),
            upper: (0, 0),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000000, 0x0001),
        Bounds {
            lower: (0x8000000000000000, 0x0001),
            upper: (0x8000000000000001, 0x0001),
            nearest_up: false,
        },
    ),
    (
        (0x0000000000000001, 0x0000),
        Bounds {
            lower: (0x0000000000000001, 0x0000),
            upper: (0x0000000000000002, 0x0000),
            nearest_up: false,
        },
    ),
    (
        (0x0000000000000003, 0x0000),
        Bounds {
            lower: (0x0000000000000003, 0x0000),
            upper: (0x0000000000000004, 0x0000),
            nearest_up: false,
        },
    ),
    (
        (0x7fffffffffffffff, 0x0000),
        Bounds {
            lower: (0x7fffffffffffffff, 0x0000),
            upper: (0x8000000000000000, 0x0001),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000000, 0x3f86),
        Bounds {
            lower: (0x8000000000000000, 0x3f86),
            upper: (0x8000000000000001, 0x3f86),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000001, 0x3f87),
        Bounds {
            lower: (0x8000000000000001, 0x3f87),
            upper: (0x8000000000000002, 0x3f87),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000000, 0x3fe1),
        Bounds {
            lower: (0x8000000000000002, 0x3fe1),
            upper: (0x8000000000000003, 0x3fe1),
            nearest_up: true,
        },
    ),
    (
        (0x8000000000000001, 0x3fde),
        Bounds {
            lower: (0x8000000000000001, 0x3fde),
            upper: (0x8000000000000002, 0x3fde),
            nearest_up: false,
        },
    ),
    (
        (0xffffffffffffffff, 0x3fde),
        Bounds {
            lower: (0xffffffffffffffff, 0x3fde),
            upper: (0x8000000000000000, 0x3fdf),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000000, 0x3fdf),
        Bounds {
            lower: (0x8000000000000000, 0x3fdf),
            upper: (0x8000000000000001, 0x3fdf),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000001, 0x3fdf),
        Bounds {
            lower: (0x8000000000000001, 0x3fdf),
            upper: (0x8000000000000002, 0x3fdf),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000000, 0x3ffd),
        Bounds {
            lower: (0x82bc2d21e262af31, 0x3ffd),
            upper: (0x82bc2d21e262af32, 0x3ffd),
            nearest_up: true,
        },
    ),
    (
        (0xffffffffffffffff, 0x3ffd),
        Bounds {
            lower: (0x8bda7adf9a3a5218, 0x3ffe),
            upper: (0x8bda7adf9a3a5219, 0x3ffe),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000000, 0x3ffe),
        Bounds {
            lower: (0x8bda7adf9a3a5218, 0x3ffe),
            upper: (0x8bda7adf9a3a5219, 0x3ffe),
            nearest_up: true,
        },
    ),
    (
        (0xc90fdaa22168c234, 0x3ffe),
        Bounds {
            lower: (0xfffffffffffffffe, 0x3ffe),
            upper: (0xffffffffffffffff, 0x3ffe),
            nearest_up: false,
        },
    ),
    (
        (0xc90fdaa22168c235, 0x3ffe),
        Bounds {
            lower: (0x8000000000000000, 0x3fff),
            upper: (0x8000000000000001, 0x3fff),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000000, 0x3fff),
        Bounds {
            lower: (0xc75922e5f71d2dc5, 0x3fff),
            upper: (0xc75922e5f71d2dc6, 0x3fff),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000001, 0x3fff),
        Bounds {
            lower: (0xc75922e5f71d2dc8, 0x3fff),
            upper: (0xc75922e5f71d2dc9, 0x3fff),
            nearest_up: true,
        },
    ),
    (
        (0xc90fdaa22168c234, 0x3fff),
        Bounds {
            lower: (0xaaaaaaaaaaaaaaaa, 0x403e),
            upper: (0xaaaaaaaaaaaaaaab, 0x403e),
            nearest_up: true,
        },
    ),
    (
        (0xc90fdaa22168c235, 0x3fff),
        Bounds {
            lower: (0xffffffffffffffff, 0xc03f),
            upper: (0x8000000000000000, 0xc040),
            nearest_up: true,
        },
    ),
    (
        (0xc90fdaa22178c234, 0x3fff),
        Bounds {
            lower: (0x8000060000480003, 0xc02a),
            upper: (0x8000060000480004, 0xc02a),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000000, 0x4000),
        Bounds {
            lower: (0x8bd7b1704a87c1da, 0xc000),
            upper: (0x8bd7b1704a87c1db, 0xc000),
            nearest_up: false,
        },
    ),
    (
        (0xc000000000000000, 0x4000),
        Bounds {
            lower: (0x91f7b892a5c37866, 0xbffc),
            upper: (0x91f7b892a5c37867, 0xbffc),
            nearest_up: false,
        },
    ),
    (
        (0xc90fdaa22168c234, 0x4000),
        Bounds {
            lower: (0xc000000000000000, 0xbfc0),
            upper: (0xc000000000000001, 0xbfc0),
            nearest_up: false,
        },
    ),
    (
        (0xc90fdaa22168c235, 0x4000),
        Bounds {
            lower: (0x8000000000000000, 0x3fbf),
            upper: (0x8000000000000001, 0x3fbf),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000000, 0x4001),
        Bounds {
            lower: (0x94337cdf26f09b86, 0x3fff),
            upper: (0x94337cdf26f09b87, 0x3fff),
            nearest_up: true,
        },
    ),
    (
        (0xfedcba9876543210, 0x4013),
        Bounds {
            lower: (0xfdae7953b4d6e641, 0x3fff),
            upper: (0xfdae7953b4d6e642, 0x3fff),
            nearest_up: true,
        },
    ),
    (
        (0x8000000000000000, 0x403b),
        Bounds {
            lower: (0xbf91c2508c643b6c, 0x3fff),
            upper: (0xbf91c2508c643b6d, 0x3fff),
            nearest_up: true,
        },
    ),
    (
        (0xdeadbeef12345678, 0x403c),
        Bounds {
            lower: (0xd633d035e9608e45, 0xbffd),
            upper: (0xd633d035e9608e46, 0xbffd),
            nearest_up: false,
        },
    ),
    (
        (0x8000000000000000, 0x403d),
        Bounds {
            lower: (0x80026c6541a088b0, 0x3fff),
            upper: (0x80026c6541a088b1, 0x3fff),
            nearest_up: false,
        },
    ),
    (
        (0xffffffffffffffff, 0x403d),
        Bounds {
            lower: (0xea57f75b8bfebb70, 0x3fff),
            upper: (0xea57f75b8bfebb71, 0x3fff),
            nearest_up: false,
        },
    ),
    (
        (0xc90fdaa221684234, 0x3fff),
        Bounds {
            lower: (0xfffe80023ffca005, 0x402e),
            upper: (0xfffe80023ffca006, 0x402e),
            nearest_up: false,
        },
    ),
    (
        (0xc90fdaa221684235, 0x3fff),
        Bounds {
            lower: (0x8000400020001000, 0x402f),
            upper: (0x8000400020001001, 0x402f),
            nearest_up: false,
        },
    ),
    (
        (0xc90fdaa221684236, 0x3fff),
        Bounds {
            lower: (0x800140032007d013, 0x402f),
            upper: (0x800140032007d014, 0x402f),
            nearest_up: true,
        },
    ),
    (
        (0xc90fdaa22168c236, 0x3fff),
        Bounds {
            lower: (0xcccccccccccccccc, 0xc03d),
            upper: (0xcccccccccccccccd, 0xc03d),
            nearest_up: true,
        },
    ),
    (
        (0xc90fdaa221694234, 0x3fff),
        Bounds {
            lower: (0x8000c0012001b002, 0xc02f),
            upper: (0x8000c0012001b003, 0xc02f),
            nearest_up: true,
        },
    ),
    (
        (0xc90fdaa221694235, 0x3fff),
        Bounds {
            lower: (0xffff80003fffe000, 0xc02e),
            upper: (0xffff80003fffe001, 0xc02e),
            nearest_up: false,
        },
    ),
    (
        (0xc90fdaa221694236, 0x3fff),
        Bounds {
            lower: (0xfffd80063ff06027, 0xc02e),
            upper: (0xfffd80063ff06028, 0xc02e),
            nearest_up: false,
        },
    ),
    (
        (0xc90fdaa2a168c234, 0x3fff),
        Bounds {
            lower: (0x80000000c0000000, 0xc01f),
            upper: (0x80000000c0000001, 0xc01f),
            nearest_up: true,
        },
    ),
    (
        (0xc90fdaa2a168c235, 0x3fff),
        Bounds {
            lower: (0xffffffff7fffffff, 0xc01e),
            upper: (0xffffffff80000000, 0xc01e),
            nearest_up: true,
        },
    ),
    (
        (0xc90fdaa2a168c236, 0x3fff),
        Bounds {
            lower: (0xfffffffd80000005, 0xc01e),
            upper: (0xfffffffd80000006, 0xc01e),
            nearest_up: true,
        },
    ),
];
