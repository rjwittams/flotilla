use flotilla_protocol::HostName;

use super::{
    resolver::{AlwaysSendKeys, AlwaysWrap, CombineStrategy},
    Hop, ResolutionContext,
};

fn minimal_context() -> ResolutionContext {
    ResolutionContext {
        current_host: HostName::new("test-host"),
        current_environment: None,
        working_directory: None,
        actions: Vec::new(),
        nesting_depth: 0,
    }
}

#[test]
fn always_wrap_returns_true() {
    let strategy = AlwaysWrap;
    let hop = Hop::RemoteToHost { host: HostName::new("feta") };
    let context = minimal_context();
    assert!(strategy.should_wrap(&hop, &context));
}

#[test]
fn always_send_keys_returns_false() {
    let strategy = AlwaysSendKeys;
    let hop = Hop::RemoteToHost { host: HostName::new("feta") };
    let context = minimal_context();
    assert!(!strategy.should_wrap(&hop, &context));
}

#[test]
fn always_wrap_returns_true_for_all_hop_variants() {
    let strategy = AlwaysWrap;
    let context = minimal_context();

    let hops = [
        Hop::RemoteToHost { host: HostName::new("gouda") },
        Hop::AttachTerminal { attachable_id: crate::attachable::AttachableId::new("sess-1") },
        Hop::RunCommand { command: vec![super::Arg::Literal("echo".into())] },
    ];

    for hop in &hops {
        assert!(strategy.should_wrap(hop, &context), "AlwaysWrap should return true for {hop:?}");
    }
}

#[test]
fn always_send_keys_returns_false_for_all_hop_variants() {
    let strategy = AlwaysSendKeys;
    let context = minimal_context();

    let hops = [
        Hop::RemoteToHost { host: HostName::new("gouda") },
        Hop::AttachTerminal { attachable_id: crate::attachable::AttachableId::new("sess-1") },
        Hop::RunCommand { command: vec![super::Arg::Literal("echo".into())] },
    ];

    for hop in &hops {
        assert!(!strategy.should_wrap(hop, &context), "AlwaysSendKeys should return false for {hop:?}");
    }
}
