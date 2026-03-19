use cleat::vt::contracts::{DeterministicReplayFixture, PassthroughFixture, assert_non_replay_contract, assert_replay_capable_contract};

#[test]
fn vt_passthrough_engine_contract_is_locked() {
    assert_non_replay_contract(&PassthroughFixture);
}

#[test]
fn vt_replay_engine_contract_is_locked() {
    assert_replay_capable_contract(&DeterministicReplayFixture);
}
