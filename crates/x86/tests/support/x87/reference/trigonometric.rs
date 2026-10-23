//! 500-bit reference values after exact reduction by pi66 = 0x3243f6a8885a308d3 * 2^-64.
//! Tiny angles use analytic bounds to retain their otherwise unrepresentable tails.

use super::Bounds;

pub(crate) struct Reference {
    pub(crate) input: (u64, u16),
    pub(crate) sine: Bounds,
    pub(crate) cosine: Bounds,
}
pub(crate) const VALUES: &[Reference] = &[
    Reference {
        input: (0x0000000000000000, 0x0000),
        sine: Bounds {
            lower: (0, 0),
            upper: (0, 0),
            nearest_up: false,
        },
        cosine: Bounds {
            lower: (0x8000000000000000, 0x3fff),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: false,
        },
    },
    Reference {
        input: (0x8000000000000000, 0x0001),
        sine: Bounds {
            lower: (0x7fffffffffffffff, 0x0000),
            upper: (0x8000000000000000, 0x0001),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x0000000000000001, 0x0000),
        sine: Bounds {
            lower: (0x0000000000000000, 0x0000),
            upper: (0x0000000000000001, 0x0000),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x0000000000000003, 0x0000),
        sine: Bounds {
            lower: (0x0000000000000002, 0x0000),
            upper: (0x0000000000000003, 0x0000),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x7fffffffffffffff, 0x0000),
        sine: Bounds {
            lower: (0x7ffffffffffffffe, 0x0000),
            upper: (0x7fffffffffffffff, 0x0000),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x8000000000000000, 0x3f86),
        sine: Bounds {
            lower: (0xffffffffffffffff, 0x3f85),
            upper: (0x8000000000000000, 0x3f86),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x8000000000000001, 0x3f87),
        sine: Bounds {
            lower: (0x8000000000000000, 0x3f87),
            upper: (0x8000000000000001, 0x3f87),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x8000000000000000, 0x3fe1),
        sine: Bounds {
            lower: (0xfffffffffffffffd, 0x3fe0),
            upper: (0xfffffffffffffffe, 0x3fe0),
            nearest_up: false,
        },
        cosine: Bounds {
            lower: (0xfffffffffffffff8, 0x3ffe),
            upper: (0xfffffffffffffff9, 0x3ffe),
            nearest_up: false,
        },
    },
    Reference {
        input: (0x8000000000000001, 0x3fde),
        sine: Bounds {
            lower: (0x8000000000000000, 0x3fde),
            upper: (0x8000000000000001, 0x3fde),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
    },
    Reference {
        input: (0xffffffffffffffff, 0x3fde),
        sine: Bounds {
            lower: (0xfffffffffffffffe, 0x3fde),
            upper: (0xffffffffffffffff, 0x3fde),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x8000000000000000, 0x3fdf),
        sine: Bounds {
            lower: (0xffffffffffffffff, 0x3fde),
            upper: (0x8000000000000000, 0x3fdf),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x8000000000000001, 0x3fdf),
        sine: Bounds {
            lower: (0x8000000000000000, 0x3fdf),
            upper: (0x8000000000000001, 0x3fdf),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: false,
        },
    },
    Reference {
        input: (0x8000000000000000, 0x3ffd),
        sine: Bounds {
            lower: (0xfd5776a798abb5d4, 0x3ffc),
            upper: (0xfd5776a798abb5d5, 0x3ffc),
            nearest_up: false,
        },
        cosine: Bounds {
            lower: (0xf80aa4fbef750ba7, 0x3ffe),
            upper: (0xf80aa4fbef750ba8, 0x3ffe),
            nearest_up: true,
        },
    },
    Reference {
        input: (0xffffffffffffffff, 0x3ffd),
        sine: Bounds {
            lower: (0xf57743a2582f7f42, 0x3ffd),
            upper: (0xf57743a2582f7f43, 0x3ffd),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xe0a94032dbea7ced, 0x3ffe),
            upper: (0xe0a94032dbea7cee, 0x3ffe),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x8000000000000000, 0x3ffe),
        sine: Bounds {
            lower: (0xf57743a2582f7f43, 0x3ffd),
            upper: (0xf57743a2582f7f44, 0x3ffd),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xe0a94032dbea7ced, 0x3ffe),
            upper: (0xe0a94032dbea7cee, 0x3ffe),
            nearest_up: true,
        },
    },
    Reference {
        input: (0xc90fdaa22168c234, 0x3ffe),
        sine: Bounds {
            lower: (0xb504f333f9de6483, 0x3ffe),
            upper: (0xb504f333f9de6484, 0x3ffe),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xb504f333f9de6484, 0x3ffe),
            upper: (0xb504f333f9de6485, 0x3ffe),
            nearest_up: true,
        },
    },
    Reference {
        input: (0xc90fdaa22168c235, 0x3ffe),
        sine: Bounds {
            lower: (0xb504f333f9de6484, 0x3ffe),
            upper: (0xb504f333f9de6485, 0x3ffe),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xb504f333f9de6484, 0x3ffe),
            upper: (0xb504f333f9de6485, 0x3ffe),
            nearest_up: false,
        },
    },
    Reference {
        input: (0x8000000000000000, 0x3fff),
        sine: Bounds {
            lower: (0xd76aa47848677020, 0x3ffe),
            upper: (0xd76aa47848677021, 0x3ffe),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0x8a51407da8345c91, 0x3ffe),
            upper: (0x8a51407da8345c92, 0x3ffe),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x8000000000000001, 0x3fff),
        sine: Bounds {
            lower: (0xd76aa47848677021, 0x3ffe),
            upper: (0xd76aa47848677022, 0x3ffe),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0x8a51407da8345c90, 0x3ffe),
            upper: (0x8a51407da8345c91, 0x3ffe),
            nearest_up: false,
        },
    },
    Reference {
        input: (0xc90fdaa22168c234, 0x3fff),
        sine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xbfffffffffffffff, 0x3fbf),
            upper: (0xc000000000000000, 0x3fbf),
            nearest_up: true,
        },
    },
    Reference {
        input: (0xc90fdaa22168c235, 0x3fff),
        sine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0xbfbd),
            upper: (0x8000000000000000, 0xbfbe),
            nearest_up: true,
        },
    },
    Reference {
        input: (0xc90fdaa22178c234, 0x3fff),
        sine: Bounds {
            lower: (0xffffffffffffffff, 0x3ffe),
            upper: (0x8000000000000000, 0x3fff),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xfffff3ffffffffff, 0xbfd3),
            upper: (0xfffff40000000000, 0xbfd3),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x8000000000000000, 0x4000),
        sine: Bounds {
            lower: (0xe8c7b7568da22efd, 0x3ffe),
            upper: (0xe8c7b7568da22efe, 0x3ffe),
            nearest_up: false,
        },
        cosine: Bounds {
            lower: (0xd51132ba9b902521, 0xbffd),
            upper: (0xd51132ba9b902522, 0xbffd),
            nearest_up: true,
        },
    },
    Reference {
        input: (0xc000000000000000, 0x4000),
        sine: Bounds {
            lower: (0x9081c36db6aada78, 0x3ffc),
            upper: (0x9081c36db6aada79, 0x3ffc),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xfd7025f42f2e9307, 0xbffe),
            upper: (0xfd7025f42f2e9308, 0xbffe),
            nearest_up: true,
        },
    },
    Reference {
        input: (0xc90fdaa22168c234, 0x4000),
        sine: Bounds {
            lower: (0xbfffffffffffffff, 0x3fc0),
            upper: (0xc000000000000000, 0x3fc0),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0xbffe),
            upper: (0x8000000000000000, 0xbfff),
            nearest_up: true,
        },
    },
    Reference {
        input: (0xc90fdaa22168c235, 0x4000),
        sine: Bounds {
            lower: (0xffffffffffffffff, 0xbfbe),
            upper: (0x8000000000000000, 0xbfbf),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xffffffffffffffff, 0xbffe),
            upper: (0x8000000000000000, 0xbfff),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x8000000000000000, 0x4001),
        sine: Bounds {
            lower: (0xc1bdceeee0f57386, 0xbffe),
            upper: (0xc1bdceeee0f57387, 0xbffe),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xa7553036d9260623, 0xbffe),
            upper: (0xa7553036d9260624, 0xbffe),
            nearest_up: false,
        },
    },
    Reference {
        input: (0xfedcba9876543210, 0x4013),
        sine: Bounds {
            lower: (0xe48dd7c2fb35ade1, 0xbffe),
            upper: (0xe48dd7c2fb35ade2, 0xbffe),
            nearest_up: false,
        },
        cosine: Bounds {
            lower: (0xe6a49453f19724da, 0xbffd),
            upper: (0xe6a49453f19724db, 0xbffd),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x8000000000000000, 0x403b),
        sine: Bounds {
            lower: (0xd4db8772945d81df, 0xbffe),
            upper: (0xd4db8772945d81e0, 0xbffe),
            nearest_up: false,
        },
        cosine: Bounds {
            lower: (0x8e3958d8ce96f570, 0xbffe),
            upper: (0x8e3958d8ce96f571, 0xbffe),
            nearest_up: false,
        },
    },
    Reference {
        input: (0xdeadbeef12345678, 0x403c),
        sine: Bounds {
            lower: (0xc59b22d0b73f94cd, 0x3ffd),
            upper: (0xc59b22d0b73f94ce, 0x3ffd),
            nearest_up: true,
        },
        cosine: Bounds {
            lower: (0xec2a45c2ad24fd6f, 0xbffe),
            upper: (0xec2a45c2ad24fd70, 0xbffe),
            nearest_up: true,
        },
    },
    Reference {
        input: (0x8000000000000000, 0x403d),
        sine: Bounds {
            lower: (0xb506a9dd559cf157, 0xbffe),
            upper: (0xb506a9dd559cf158, 0xbffe),
            nearest_up: false,
        },
        cosine: Bounds {
            lower: (0xb5033c8677153459, 0xbffe),
            upper: (0xb5033c867715345a, 0xbffe),
            nearest_up: true,
        },
    },
    Reference {
        input: (0xffffffffffffffff, 0x403d),
        sine: Bounds {
            lower: (0xe0ab9300da6d2684, 0x3ffe),
            upper: (0xe0ab9300da6d2685, 0x3ffe),
            nearest_up: false,
        },
        cosine: Bounds {
            lower: (0xf56ec1e0a37c4176, 0x3ffd),
            upper: (0xf56ec1e0a37c4177, 0x3ffd),
            nearest_up: false,
        },
    },
];
