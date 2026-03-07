⏺ Good. So the generic nouns from the label system are:
  - checkouts: "checkout" (git), "worktree" (git worktree manager)
  - code_review: "pull request" / "PR" (GitHub)
  - issues: "issue"
  - sessions: "session"

  Here's my audit of all Command variants, Intent variants, and CommandResult variants:

  Current State: Command, Intent, and CommandResult Naming Audit

  Commands

  ┌───────────────────────────────────────┬──────────────────────────────────────────────┬───────────────────────────────────────────────────────────┬─────────────────────────────────────────────┐
  │             Current Name              │                 What it does                 │                          Problem                          │               Suggested Name                │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ SwitchWorktree { path }               │ Creates a workspace (tmux/zellij) for an     │ Name says "switch worktree" but it creates a workspace.   │ CreateWorkspaceForCheckout                  │
  │                                       │ existing checkout                            │ Hardcodes "worktree".                                     │                                             │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ SelectWorkspace { ws_ref }            │ Switches to an existing workspace            │ Fine — "workspace" is generic                             │ ✅ Keep                                     │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ CreateWorktree { branch, ... }        │ Creates a checkout + workspace, optionally   │ Hardcodes "worktree". Also does two things (create        │ CreateCheckout                              │
  │                                       │ linking issues                               │ checkout + workspace).                                    │                                             │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ RemoveCheckout { branch }             │ Removes a checkout                           │ ✅ Already uses generic "checkout"                        │ ✅ Keep                                     │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ FetchDeleteInfo { branch,             │ Fetches safety info before deleting a        │ worktree_path field hardcodes "worktree"                  │ FetchDeleteInfo { branch, checkout_path,    │
  │ worktree_path, ... }                  │ checkout                                     │                                                           │ ... }                                       │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ OpenPr { id }                         │ Opens a change request in the browser        │ Hardcodes "PR"                                            │ OpenChangeRequest                           │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ OpenIssueBrowser { id }               │ Opens an issue in the browser                │ Awkward name — "Browser" is an implementation detail      │ OpenIssue                                   │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ LinkIssuesToPr { pr_id, ... }         │ Links issues to a change request body        │ Hardcodes "PR"                                            │ LinkIssuesToChangeRequest {                 │
  │                                       │                                              │                                                           │ change_request_id, ... }                    │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ ArchiveSession { session_id }         │ Archives a coding agent session              │ ✅ "session" is generic                                   │ ✅ Keep                                     │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ GenerateBranchName { issue_keys }     │ AI-generates a branch name from issues       │ ✅ Fine                                                   │ ✅ Keep                                     │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ TeleportSession { session_id, ... }   │ Teleports to a coding agent session in a     │ ✅ Fine                                                   │ ✅ Keep                                     │
  │                                       │ workspace                                    │                                                           │                                             │
  ├───────────────────────────────────────┼──────────────────────────────────────────────┼───────────────────────────────────────────────────────────┼─────────────────────────────────────────────┤
  │ AddRepo / RemoveRepo / Refresh        │ Daemon-level repo management                 │ ✅ Fine                                                   │ ✅ Keep                                     │
  └───────────────────────────────────────┴──────────────────────────────────────────────┴───────────────────────────────────────────────────────────┴─────────────────────────────────────────────┘

  Intents

  ┌────────────────────────────┬──────────────────────┬────────────────────────────┐
  │        Current Name        │       Problem        │       Suggested Name       │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ SwitchToWorkspace          │ ✅ Fine              │ ✅ Keep                    │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ CreateWorkspace            │ ✅ Fine              │ ✅ Keep                    │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ RemoveWorktree             │ Hardcodes "worktree" │ RemoveCheckout             │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ CreateWorktreeAndWorkspace │ Hardcodes "worktree" │ CreateCheckoutAndWorkspace │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ GenerateBranchName         │ ✅ Fine              │ ✅ Keep                    │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ OpenPr                     │ Hardcodes "PR"       │ OpenChangeRequest          │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ OpenIssue                  │ ✅ Fine              │ ✅ Keep                    │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ LinkIssuesToPr             │ Hardcodes "PR"       │ LinkIssuesToChangeRequest  │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ TeleportSession            │ ✅ Fine              │ ✅ Keep                    │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ ArchiveSession             │ ✅ Fine              │ ✅ Keep                    │
  └────────────────────────────┴──────────────────────┴────────────────────────────┘

  CommandResult

  ┌────────────────────────────┬──────────────────────┬────────────────────────────┐
  │        Current Name        │       Problem        │       Suggested Name       │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ Ok                         │ ✅ Fine              │ ✅ Keep                    │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ WorktreeCreated { branch } │ Hardcodes "worktree" │ CheckoutCreated { branch } │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ BranchNameGenerated        │ ✅ Fine              │ ✅ Keep                    │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ DeleteInfo(DeleteInfo)     │ ✅ Fine              │ ✅ Keep                    │
  ├────────────────────────────┼──────────────────────┼────────────────────────────┤
  │ Error { message }          │ ✅ Fine              │ ✅ Keep                    │
  └────────────────────────────┴──────────────────────┴────────────────────────────┘

  Summary of changes

  7 renames total:
  1. Command::SwitchWorktree → CreateWorkspaceForCheckout
  2. Command::CreateWorktree → CreateCheckout
  3. Command::FetchDeleteInfo.worktree_path → checkout_path
  4. Command::OpenPr → OpenChangeRequest
  5. Command::OpenIssueBrowser → OpenIssue
  6. Command::LinkIssuesToPr → LinkIssuesToChangeRequest (field pr_id → change_request_id)
  7. CommandResult::WorktreeCreated → CheckoutCreated

  5 Intent renames:
  1. Intent::RemoveWorktree → RemoveCheckout
  2. Intent::CreateWorktreeAndWorkspace → CreateCheckoutAndWorkspace
  3. Intent::OpenPr → OpenChangeRequest
  4. Intent::LinkIssuesToPr → LinkIssuesToChangeRequest
