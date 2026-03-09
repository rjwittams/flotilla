mod test_fixtures;

use flotilla_protocol::{ProviderData, SessionStatus};
use test_fixtures::*;

#[test]
fn empty_state() {
    let mut harness = TestHarness::empty();
    let output = harness.render_to_string();
    insta::assert_snapshot!(output);
}

#[test]
fn single_repo_empty_table() {
    let mut harness = TestHarness::single_repo("my-project");
    let output = harness.render_to_string();
    insta::assert_snapshot!(output);
}

#[test]
fn single_repo_with_items() {
    let mut providers = ProviderData::default();
    let (path, checkout) = make_checkout("feat-login", "/test/my-project/feat-login", false);
    providers.checkouts.insert(path, checkout);
    let (id, cr) = make_change_request("42", "Add login page", "feat-login");
    providers.change_requests.insert(id, cr);
    let (id, issue) = make_issue("10", "Users need authentication");
    providers.issues.insert(id, issue);
    let (id, session) = make_session("s1", "Implement auth flow", SessionStatus::Idle);
    providers.sessions.insert(id, session);

    let items = vec![
        make_work_item_checkout("feat-login", "/test/my-project/feat-login"),
        make_work_item_cr("42", "Add login page"),
        make_work_item_issue("10", "Users need authentication"),
    ];

    let mut harness = TestHarness::single_repo("my-project").with_provider_data(providers, items);
    let output = harness.render_to_string();
    insta::assert_snapshot!(output);
}
