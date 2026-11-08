//! Wire commands, replies and flags shared by devices and firmware consumers.

pub const ACK: u8 = 0xfa;
pub const RESEND: u8 = 0xfe;
pub const SELF_TEST_PASSED: u8 = 0xaa;

pub mod controller {
    pub const READ_COMMAND_BYTE: u8 = 0x20;
    pub const WRITE_COMMAND_BYTE: u8 = 0x60;
    pub const SELF_TEST: u8 = 0xaa;
    pub const KEYBOARD_TEST: u8 = 0xab;
    pub const DISABLE_KEYBOARD: u8 = 0xad;
    pub const ENABLE_KEYBOARD: u8 = 0xae;
    pub const DISABLE_AUXILIARY: u8 = 0xa7;
    pub const ENABLE_AUXILIARY: u8 = 0xa8;
    pub const AUXILIARY_TEST: u8 = 0xa9;
    pub const WRITE_KEYBOARD_OUTPUT: u8 = 0xd2;
    pub const WRITE_AUXILIARY_OUTPUT: u8 = 0xd3;
    pub const WRITE_AUXILIARY: u8 = 0xd4;

    // Command-byte flags; SYSTEM_INITIALIZED also appears in the status register.
    pub const KEYBOARD_IRQ_ENABLED: u8 = 1;
    pub const AUXILIARY_IRQ_ENABLED: u8 = 1 << 1;
    pub const SYSTEM_INITIALIZED: u8 = 1 << 2;
    pub const KEYBOARD_DISABLED: u8 = 1 << 4;
    pub const AUXILIARY_DISABLED: u8 = 1 << 5;
    pub const TRANSLATE_SET_2: u8 = 1 << 6;
}

pub mod mouse {
    pub const RESET: u8 = 0xff;
    pub const SET_DEFAULTS: u8 = 0xf6;
    pub const DISABLE_REPORTING: u8 = 0xf5;
    pub const ENABLE_REPORTING: u8 = 0xf4;
    pub const SET_SAMPLE_RATE: u8 = 0xf3;
    pub const IDENTIFY: u8 = 0xf2;
    pub const SET_REMOTE_MODE: u8 = 0xf0;
    pub const SET_WRAP_MODE: u8 = 0xee;
    pub const RESET_WRAP_MODE: u8 = 0xec;
    pub const READ_DATA: u8 = 0xeb;
    pub const SET_STREAM_MODE: u8 = 0xea;
    pub const STATUS: u8 = 0xe9;
    pub const SET_RESOLUTION: u8 = 0xe8;
    pub const SCALING_2_TO_1: u8 = 0xe7;
    pub const SCALING_1_TO_1: u8 = 0xe6;

    pub const DEVICE_ID: u8 = 0;
    pub const ERROR: u8 = 0xfc;
    /// Always set in the first byte of a movement packet.
    pub const PACKET_HEADER: u8 = 1 << 3;
}
