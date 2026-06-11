@ISSUE-68 @adversarial-qa
Feature: subscription_audit dedup across the 12-month window (Gate 3 re-attack)

  The window fix (ADR 0015 Decision 7) widened the audit fetch to 12 months so
  non-monthly streams are captured. That makes a monthly stream appear ~12 times
  in the window; get_recurring_for_audit must dedup by (merchant, stream_amount)
  so the stream is counted ONCE, not 12x. These scenarios exercise that dedup
  through the real MCP boundary using only committed step definitions. The
  committed @ISSUE-68 scenarios never hit the dedup path (every one uses distinct
  merchants), so this is the missing hermetic coverage for the linchpin of the fix.

  Background:
    Given the budgeting advisor is connected to the household's finances

  @ISSUE-68 @adversarial-qa
  Scenario: Three occurrences of one monthly stream are counted once (dedup collapse)
    # If dedup is broken, total_monthly would be 45.00 (3x) instead of 15.00.
    Given the household has the following recurring charges this period:
      | merchant | stream_amount | frequency | is_approximate | is_past |
      | Netflix  | 15.00         | monthly   | false          | false   |
      | Netflix  | 15.00         | monthly   | false          | false   |
      | Netflix  | 15.00         | monthly   | false          | false   |
    When the advisor requests subscription_audit
    Then the subscription list contains Netflix
    And total_monthly is 15.00
    And total_annual is 180.00

  @ISSUE-68 @adversarial-qa
  Scenario: Same merchant at two different amounts stays separate (dedup keys on amount)
    # Both Apple streams must survive: distinct amounts => distinct dedup keys.
    # total_monthly = 0.99 + 9.99 = 10.98 proves both are present and counted.
    Given the household has the following recurring charges this period:
      | merchant | stream_amount | frequency | is_approximate | is_past |
      | Apple    | 0.99          | monthly   | false          | false   |
      | Apple    | 9.99          | monthly   | false          | false   |
    When the advisor requests subscription_audit
    Then the subscription list contains Apple
    And total_monthly is 10.98
    And total_annual is 131.76
