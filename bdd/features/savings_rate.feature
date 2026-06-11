@ISSUE-65
Feature: savings_rate reports monthly income, true spending, and savings rate

  Background:
    Given the budgeting advisor is connected to the household's finances

  @ISSUE-65
  Scenario: A month with income and expenses reports the correct savings rate
    Given a month 2026-04 with 6000 income and 4500 true spending
    When the advisor requests savings_rate for 2026-04-01 to 2026-04-30
    Then the month 2026-04 reports income 6000
    And the month 2026-04 reports true_spending 4500
    And the month 2026-04 reports net_savings 1500
    And the month 2026-04 reports a savings_rate of 25 percent

  @ISSUE-65
  Scenario: Transfers and credit-card payments are excluded from true spending
    Given a month 2026-04 with 6000 income, 4000 expenses, and a 2000 credit-card payment
    When the advisor requests savings_rate for 2026-04-01 to 2026-04-30
    Then the month 2026-04 reports true_spending 4000
    And the credit-card payment is not counted in true_spending

  @ISSUE-65
  Scenario: A multi-month window reports an average savings rate
    Given three complete months of income and spending starting 2026-02
    When the advisor requests savings_rate with months 3
    Then three months are returned
    And a window-average savings_rate is reported

  @ISSUE-65
  Scenario: The response contains aggregates not raw transactions
    Given a month 2026-04 with 6000 income and 4500 true spending
    When the advisor requests savings_rate for 2026-04-01 to 2026-04-30
    Then the response contains no individual transaction list

  @ISSUE-65
  Scenario: Zero income month reports no savings rate
    Given a month 2026-04 with 0 income and 2000 true spending
    When the advisor requests savings_rate for 2026-04-01 to 2026-04-30
    Then the month 2026-04 reports income 0
    And the month 2026-04 savings_rate is absent
