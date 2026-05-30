@ISSUE-A7
Feature: Progress versus goals
  The tool that makes the advisor an advisor rather than a reporter. It
  measures the household's actual finances against the goals it remembers
  — savings rate, emergency-fund runway, debt payoff — and reports whether
  each goal is on track, drifting, or off, so guidance is always anchored
  to what the household is actually trying to achieve.

  Each goal is classified by the same banding: on track at or above the
  goal, off below half the goal, and drifting anywhere in between.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: Savings-rate progress is judged against the stored goal

    Scenario Outline: Savings rate is classified against the goal
      Given the household's savings-rate goal is 20 percent
      And the household's actual savings rate is <actual> percent
      When the advisor reviews progress versus goals
      Then the savings-rate goal is reported as <status>

      Examples:
        | actual | status   |
        | 25     | on track |
        | 20     | on track |
        | 17     | drifting |
        | 10     | drifting |
        | 6      | off      |

  Rule: Emergency-fund runway is judged against the target

    Scenario Outline: Emergency-fund runway is classified against the target
      Given the household's emergency-fund goal is 6 months of expenses
      And the household's cash reserves cover <months> months of expenses
      When the advisor reviews progress versus goals
      Then the emergency-fund goal is reported as <status>

      Examples:
        | months | status   |
        | 7      | on track |
        | 6      | on track |
        | 4      | drifting |
        | 2      | off      |

  Rule: Progress is reported only for goals the household has set

    Scenario: A goal that has not been set is not reported
      Given the household has not set a debt-payoff goal
      When the advisor reviews progress versus goals
      Then no debt-payoff progress is reported

  Rule: An expired session is reported as needing re-authentication

    Scenario: An expired session asks the household to re-authenticate
      Given the household's Monarch session has expired
      When the advisor reviews progress versus goals
      Then the advisor reports that re-authentication is required
