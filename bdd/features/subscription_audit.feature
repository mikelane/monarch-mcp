@ISSUE-68
Feature: subscription_audit ranks recurring costs and annualizes the total

  Background:
    Given the budgeting advisor is connected to the household's finances

  @ISSUE-68
  Scenario: Subscriptions are ranked by annualized cost
    Given the household has the following recurring charges this period:
      | merchant        | stream_amount | frequency | is_approximate | is_past |
      | StreamBundle    | 50.00         | monthly   | false          | false   |
      | NewsCo          | 120.00        | yearly    | false          | false   |
    When the advisor requests subscription_audit
    Then the subscription list contains StreamBundle
    And the subscription list contains NewsCo
    And StreamBundle is ranked above NewsCo
    And StreamBundle annualized_amount is 600.00
    And NewsCo annualized_amount is 120.00

  @ISSUE-68
  Scenario: A yearly charge is normalized to a monthly-equivalent
    Given the household has the following recurring charges this period:
      | merchant | stream_amount | frequency | is_approximate | is_past |
      | NewsCo   | 120.00        | yearly    | false          | false   |
    When the advisor requests subscription_audit
    Then the NewsCo monthly_amount is 10.00

  @ISSUE-68
  Scenario: The audit reports total monthly and annual burn
    Given the household has the following recurring charges this period:
      | merchant     | stream_amount | frequency | is_approximate | is_past |
      | StreamBundle | 50.00         | monthly   | false          | false   |
      | NewsCo       | 120.00        | yearly    | false          | false   |
    When the advisor requests subscription_audit
    Then total_monthly is 60.00
    And total_annual is 720.00

  @ISSUE-68
  Scenario: Income streams are excluded from the subscription burn
    Given the household has the following recurring charges this period:
      | merchant     | stream_amount | frequency | is_approximate | is_past |
      | StreamBundle | 50.00         | monthly   | false          | false   |
    And the household has the following income streams:
      | merchant | stream_amount | frequency |
      | Employer | 5000.00       | monthly   |
    When the advisor requests subscription_audit
    Then the subscription list contains StreamBundle
    And the subscription list does not contain Employer

  @ISSUE-68
  Scenario: Approximate streams are included but flagged
    Given the household has the following recurring charges this period:
      | merchant     | stream_amount | frequency | is_approximate | is_past |
      | ElectricUtil | 120.00        | monthly   | true           | false   |
    When the advisor requests subscription_audit
    Then the subscription list contains ElectricUtil
    And ElectricUtil is flagged as approximate

  @ISSUE-68
  Scenario: Empty recurring items produce a zero result
    Given the household has no recurring charges this period
    When the advisor requests subscription_audit
    Then the subscription list is empty
    And total_monthly is 0.00
    And total_annual is 0.00

  @ISSUE-68
  Scenario: Session expiry surfaces a re-authentication message
    Given the household's Monarch session has expired
    When the advisor requests subscription_audit
    Then the advisor reports that re-authentication is required
