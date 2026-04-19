mod fixture;
mod protocol;
mod state;
mod udp;

pub use fixture::{
    MOCK_EVENT_CHANNEL, MOCK_EVENT_NAME, MOCK_ID_STRING, MOCK_SEGMENT_PARAMS, MOCK_SLAVE_ADDR,
    PARAM_COUNTER_MAX_ADDR, PARAM_MATRIX_ADDR, PARAM_MATRIX_COLS, PARAM_MATRIX_LEN,
    PARAM_MATRIX_ROWS,
};
pub use protocol::{build_frame, parse_frame};
pub use udp::MockXcpSlaveServer;
