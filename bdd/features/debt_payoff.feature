@ISSUE-27
Feature: Debt-payoff progress
  The advisor measures how a household is tracking toward paying off its debts
  by the target date. Two complementary signals are computed: whether the
  monthly payment plan is sufficient to clear the debt on time (on_schedule),
  and whether the household's actual behaviour last month matched the plan
  (on_pace). The overall status is the worst of the two computable signals.

  MONARCH_NOW is pinned to 2026-05-15 in environment.py, so:
    - target_date = "2027-01-01" → 8 months to target
    - target_date = "2026-04-01" → target has passed (months_to_target ≤ 0)

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: A household paying enough to clear debt by the target date is on schedule

    Scenario: Household on schedule to clear debt by target date
      Given the household's debt-payoff goal targets "2027-01-01" with 1000 per month
      And the household has a credit account with a balance of 7000 owed
      And the household paid down 1000 in debt last month
      When the advisor reviews progress versus goals
      Then the debt-payoff progress is reported as "on track"
      And the debt-payoff total_debt is 7000.0
      And the months_to_target is 8
      And the required_monthly is approximately 875.0
      And the on_schedule signal is "on track"
      And the on_pace signal is "on track"

    Scenario: Household paying less than required amount is drifting
      Given the household's debt-payoff goal targets "2027-01-01" with 500 per month
      And the household has a credit account with a balance of 7000 owed
      And the household paid down 500 in debt last month
      When the advisor reviews progress versus goals
      Then the debt-payoff progress is reported as "drifting"
      And the on_schedule signal is "off"
      And the on_pace signal is "on track"

    Scenario: Household paying far too little is off schedule
      Given the household's debt-payoff goal targets "2027-01-01" with 200 per month
      And the household has a credit account with a balance of 7000 owed
      And the household paid down 200 in debt last month
      When the advisor reviews progress versus goals
      Then the debt-payoff progress is reported as "off"
      And the on_schedule signal is "off"
      And the on_pace signal is "off"

  Rule: A debt-only config returns debt_payoff progress, not guidance

    Scenario: Debt-only config returns debt_payoff field instead of guidance
      Given the household's debt-payoff goal targets "2027-01-01" with 1000 per month
      And the household has a credit account with a balance of 7000 owed
      And the household paid down 1000 in debt last month
      When the advisor reviews progress versus goals
      Then the response contains a "debt_payoff" field
      And the response does not contain a "guidance" field

  Rule: Already paid-off debt is on track

    Scenario: Household with no debt remaining is on track
      Given the household's debt-payoff goal targets "2027-01-01" with 500 per month
      And the household has no debt accounts
      When the advisor reviews progress versus goals
      Then the debt-payoff progress is reported as "on track"
      And the debt-payoff total_debt is 0.0

  Rule: Target date in the past with remaining debt is off

    Scenario: Target date has passed with debt remaining
      Given the household's debt-payoff goal targets "2026-04-01" with 500 per month
      And the household has a credit account with a balance of 3000 owed
      When the advisor reviews progress versus goals
      Then the debt-payoff progress is reported as "off"
      And the on_schedule signal is "off"

  Rule: Missing monthly_payment means on_schedule and on_pace are unknown

    Scenario: No monthly_payment configured — numbers still surface
      Given the household's debt-payoff goal targets "2027-01-01" with no monthly payment
      And the household has a credit account with a balance of 7000 owed
      When the advisor reviews progress versus goals
      Then the debt-payoff total_debt is 7000.0
      And the months_to_target is 8
      And no on_schedule signal is reported
      And no on_pace signal is reported
