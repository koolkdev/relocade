//! Bitwise operations use all 128 bits without floating-point interpretation.

use super::*;

instruction_families! {
    ANDPS {
        execute: binary(|left, right| left.and(right));
        forms { NP 0x0f 0x54 => operands(xmm, xmm_rm); }
    }
    ANDPD {
        execute: binary(|left, right| left.and(right));
        forms { P66 0x0f 0x54 => operands(xmm, xmm_rm); }
    }
    ANDNPS {
        execute: binary(|left, right| left.xor(u128::MAX).and(right));
        forms { NP 0x0f 0x55 => operands(xmm, xmm_rm); }
    }
    ANDNPD {
        execute: binary(|left, right| left.xor(u128::MAX).and(right));
        forms { P66 0x0f 0x55 => operands(xmm, xmm_rm); }
    }
    ORPS {
        execute: binary(|left, right| left.or(right));
        forms { NP 0x0f 0x56 => operands(xmm, xmm_rm); }
    }
    ORPD {
        execute: binary(|left, right| left.or(right));
        forms { P66 0x0f 0x56 => operands(xmm, xmm_rm); }
    }
    XORPS {
        execute: binary(|left, right| left.xor(right));
        forms { NP 0x0f 0x57 => operands(xmm, xmm_rm); }
    }
    XORPD {
        execute: binary(|left, right| left.xor(right));
        forms { P66 0x0f 0x57 => operands(xmm, xmm_rm); }
    }
    PAND {
        execute: binary(|left, right| left.and(right));
        forms { P66 0x0f 0xdb => operands(xmm, xmm_rm); }
    }
    PANDN {
        execute: binary(|left, right| left.xor(u128::MAX).and(right));
        forms { P66 0x0f 0xdf => operands(xmm, xmm_rm); }
    }
    POR {
        execute: binary(|left, right| left.or(right));
        forms { P66 0x0f 0xeb => operands(xmm, xmm_rm); }
    }
    PXOR {
        execute: binary(|left, right| left.xor(right));
        forms { P66 0x0f 0xef => operands(xmm, xmm_rm); }
    }
}
