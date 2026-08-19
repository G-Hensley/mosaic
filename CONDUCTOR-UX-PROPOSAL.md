# Conductor UX Redesign Proposal

## Executive Summary

This proposal addresses two critical UX problems in Mosaic's conductor interface:

1. **Blocked Session Detection & Display**: Sessions waiting for human input (e.g., approval prompts) are indistinguishable from busy sessions, causing silent stalls.
2. **Conductor Task Strip Scaling**: The current horizontal pill strip doesn't scale beyond 5 concurrent tasks and truncates distinguishing information.

The proposed solution is a **Trello-style task board** (aligned with the IMPROVEMENT-AUDIT.md Section 16) that simultaneously solves both problems through:
- Reusing the existing quiet-detection signal from `lib.rs` for blocked session detection
- Implementing a visual sorting mechanism that surfaces blocked sessions at the top
- Replacing the horizontal pill strip with a scalable task board interface

---

## Problem Analysis

### Problem 1: Blocked Sessions Invisible to Conductor
**Observed Behavior**: During live testing, `sess-2` was blocked waiting for a Codex approval prompt (`cargo clippy --all-targets --all-features`) while its conductor pill remained identical to running sessions. The entire dispatch fan-out stalled until manual intervention.

**Root Cause**: Mosaic's `conductorState()` API only reports task status as `pending`/`done`/`error`/`timeout`/`cancelled`. It cannot distinguish between:
- **Busy**: Actively processing/computing (no human intervention needed)
- **Blocked**: Awaiting human input (e.g., approval, permission, clarification)

**Technical Constraint**: From the PTY layer, Mosaic only observes raw terminal output. A blocked session waiting for input produces no output, making it indistinguishable from:
- A session that's actively thinking/computing (producing no terminal output during computation)
- A session in a genuinely quiet state

**Failure Modes of Detection**:
| Detection Method | False Positive Risk | False Negative Risk | Implementation Cost |
|------------------|---------------------|---------------------|---------------------|
| Content Analysis (keyword matching) | High (output contains words like "approve" during normal operation) | Low | Medium |
| Timing-Based Quiet Detection | Medium-Low (only blocks on genuine inactivity) | Medium-Low (blocked sessions may produce periodic output) | **Low (reuse existing signal)** |
| Periodic Heartbeat Requests | Low (requires agent cooperation) | High (agents may miss heartbeats) | High |

### Problem 2: Conductor Task Strip Scaling Failure
**Current Implementation** (`ConductorBar.tsx` lines 42-62):
- Fixed-width horizontal pills
- Task text truncated to 38 characters
- Maximum 5 visible pills (older tasks inaccessible without dragging scrollbar)
- No distinction between similar task types (e.g., two "build email form" dispatches)

**Observed Pain Points**:
1. At 6+ concurrent tasks, users must hunt through a thin scrollbar
2. Truncation removes precisely the distinguishing information (parameters, targets)
3. No visual priority or status beyond basic `pending`/`done` states

---

## Proposed Solution

### Core Insight: Reuse Existing Quiet-Detection Signal
The backend already implements sophisticated timing-based dispatch detection in `lib.rs` (lines 129-142) to solve Problem #1 from the audit (dispatch submit race). This signal can be **repurposed** for Problem 1:

```rust
// From lib.rs:ready_to_submit() - exactly what we need
fn ready_to_submit(now: u64, started: u64, last_output: u64, baseline: u64) -> bool {
    let waited = now.saturating_sub(started);
    if waited >= SUBMIT_CEILING_MS { return true; }           // timeout escape
    if waited < SUBMIT_FLOOR_MS { return false; }            // minimum wait
    if last_output == baseline { return false; }             // **KEY SIGNAL: still receiving input**
    now.saturating_sub(last_output) >= SUBMIT_QUIET_MS       // quiet long enough = done
}
```

**Detection Logic**:
- When `last_output == baseline` persists beyond `SUBMIT_FLOOR_MS` (300ms), the session is either:
  1. Blocked waiting for human input (no output being generated)
  2. Actively computing with no terminal output (rare for LLMs during tool use)
- **False Positive Cost**: Very low for coding agents - they typically stream output continuously during active work (thinking tokens, tool calls, progress updates)
- **Validation**: Reusing the exact same signal eliminates disagreement between systems

### UI Implementation: Trello-Style Task Board

Replace the horizontal `ConductorBar` with a vertically scrollable task board featuring three columns:

```
�┌─────────────────────────────────────────────────────────────�┐
│                 MOSAIC CONDUCTOR TASK BOARD                 │
├───────────────�┬─────────────────────�┬───────────────────────�┤
│   �� ⚠��️ BLOCKED   │    � ▶��️ IN PROGRESS   │      � ✅ COMPLETED      │
│               │                     │                       │
│  � ┌─────────�┐  │  � ┌─────────────�┐   │  � ┌─────────────────�┐  │
│  │ sess-2  │  │  │ sess-1: ... │   │  │ sess-3: deploy  │  │
│  │ Codex:  │  │  │ Claude: bu... │   │  │ to prod (ok)  │  │
│  │ approve │  │  │ ild schema  │   │  │                 │  │
│  │ this    │  │  │             │   │  │                 │  │
│  │ action? │  │  │             │   │  │                 │  │
│  └─────────�┘  │  └─────────────�┘   │  └─────────────────┘  │
│               │  � ┌─────────────�┐   │  � ┌─────────────────�┐  │
│               │  │ sess-4: ... │   │  │ sess-5: test... │  │
│               │  │ Opencode: ...│   │  │ suite (run...)  │  │
│               │  │ tests       │   │  │                 │  │
│               │  └─────────────�┘   │  └─────────────────�┘  │
│               │                    │                     │
│               │   [+ New Task]     │     [Clear Done]      │
�└───────────────�┴────────────────────�┴───────────────────────�┘
```

#### Column Definitions:
1. **��⚠��️ BLOCKED** (Top-Aligned Priority)
   - Sessions detected as blocked via `last_output == baseline` signal
   - Auto-sorts to top of column by detection timestamp (oldest first)
   - Visual treatment: Warm amber background (`--amber:#f0a868`), pulse animation
   - Tooltip: "Waiting for human input: [specific prompt if detectable]"
   - Actions: Click to focus session, right-click for "Force Resume" (with warning)

2. **�▶��️ IN PROGRESS** (Default State)
   - Tasks with normal progress (output changing regularly)
   - Sorted by dispatch timestamp (newest first)
   - Visual treatment: Standard theme background
   - Shows: Agent icon, truncated task (50 chars), elapsed time
   - Tooltip: Full task details on hover

3. **��✅ COMPLETED** (Bottom-Aligned Archive)
   - Tasks with status `done`/`error`/`timeout`/`cancelled`
   - Sorted by completion timestamp (newest first)
   - Visual treatment: Muted theme background
   - Shows: Agent icon, result status, duration
   - Tooltip: Full result/error details
   - Bulk action: "Clear Done" button

#### Key Improvements Over Current Strip:
| Feature | Current Strip | Proposed Board | Benefit |
|---------|---------------|----------------|---------|
| Blocked Detection | �� ❌ Invisible | � ✅ Auto-surfaces to top | Prevents silent stalls |
| Visual Priority | �� ❌ All equal | � ✅ Blocked = top priority | Immediate attention |
| Task Visibility | �� ❌ Max 5 visible | � ✅ Vertical scrolling | No hidden tasks |
| Text Truncation | �� ❌ 38 chars | � ✅ 50-70 chars + tooltips | Preserves distinguishing info |
| Status Granularity | �� ❌ pending/done | � ✅ 3-state workflow | Clear progress tracking |
| Bulk Operations | �� ❌ None | � ✅ Clear Done | Efficient cleanup |

### Technical Implementation Plan

#### Backend Changes (`src-tauri/src/lib.rs`):
1. Extend `ConductorTask` status enum to include `blocked` variant
2. Modify `conductorState()` to return blocked status when:
   - `last_output == baseline` AND
   - `waited > SUBMIT_FLOOR_MS` AND
   - Task is in `pending` state
3. Add telemetry to track false positive/negative rates for tuning

#### Frontend Changes:
1. Replace `ConductorBar.tsx` with `ConductorBoard.tsx`
2. Update `App.tsx` to use new component
3. Add board-specific styling to `App.css`
4. Implement column logic and drag/drop (for future task reassignment)

#### Styling Considerations:
- Reuse existing theme tokens from `themes.ts`
- Blocked state: Warm amber accent (from Atelier mock: `--amber:#f0a868`)
- Maintain accessibility: ARIA labels, keyboard navigation
- Responsive design: Collapsible columns on narrow screens

---

## Evaluation Against Audit Findings

### Does the Task Board Subsume Existing Problems?

| Audit Finding | How Task Board Addresses It | Status |
|---------------|-----------------------------|--------|
| **#10 No dispatch UI** | Board *is* the dispatch UI (human-initiated task creation) | **SUBSUMED** |
| **#11 No completion notifications** | Task moving to Done column = explicit notification | **SUBSUMED** |
| **#12 No worktree diff/merge surface** | Review column (future extension) forces diff surface | **FUTURE-EXTENSION** |
| **#13 Task authorization missing** | Card requires explicit `owner` field | **FUTURE-EXTENSION** |
| **#15 Dispatch needs structured schema** | Card *is* structured task (title, owner, state, result) | **SUBSUMED** |
| **#4 Shared brain persistence** | Board persistence requires durable storage | **FUTURE-EXTENSION** |

**Verdict**: The task board **subsumes Problems #10, #11, and partially #15** from the audit. It provides the foundation for addressing #12, #13, and #4 in future work.

### Risk Assessment
| Risk | Mitigation |
|------|------------|
| False positive blocked sessions | Reuse existing battle-tested signal from dispatch timing logic |
| Development complexity | Start with blocked detection + basic board; iterate |
| Performance impact | Minimal (same signal computation, virtualized list rendering) |
| User learning curve | Familiar Trello/Kanban pattern; tooltips for guidance |

---

## First-Day Implementation Plan (Vertical Slice)

If limited to one day of implementation, deliver this **minimal viable solution**:

1. **Backend** (`lib.rs`):
   - Add `Blocked` variant to `ConductorTaskStatus` enum
   - Modify `conductorState()` to detect blocked sessions using existing quiet logic
   - Return blocked tasks with new status

2. **Frontend**:
   - Create `ConductorBoard.tsx` with three hardcoded columns
   - Implement blocked session detection from API response
   - Show blocked sessions in top column with visual treatment
   - Show normal pending tasks in middle column
   - Show completed tasks in bottom column
   - Apply existing theme tokens for styling
   - Keep current `ConductorBar` as fallback for comparison

**Success Criteria for Day One**:
- [ ] Blocked session auto-appears in top column when detected
- [ ] Normal tasks appear in middle column
- [ ] Completed tasks appear in bottom column
- [ ] Visual distinction: blocked sessions use warm amber accent
- [ ] No regressions in existing conductor functionality
- [ ] `pnpm build` passes

This delivers immediate value for Problem #1 while establishing the foundation for the full board solution addressing Problem #2.

---

## Conclusion

The Trello-style task board is not merely a UI enhancement—it's the **natural architectural evolution** that solves multiple interconnected problems:
- Transforms invisible blocked sessions into actionable priorities
- Replaces an unscalable status strip with a genuine workflow visualization
- Provides the foundation for human-initiated dispatch (Audit #10)
- Creates the substrate for task completion notifications (Audit #11)
- Establishes the groundwork for structured task schemas (Audit #15)

By reusing the existing quiet-detection signal from `lib.rs`, we ensure technical consistency while avoiding the pitfall of duplicated logic that could produce conflicting signals. The proposal accepts that perfect blocked-session detection is impossible from the PTY layer, but leverages the best available signal with clearly understood failure modes.

This approach delivers immediate user value while setting the stage for the broader workflow improvements outlined in the audit document.