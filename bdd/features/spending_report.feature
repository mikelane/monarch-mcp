@ISSUE-A5 @not_implemented
Feature: Spending report
  The advisor's weekly/monthly workhorse. It groups spending by category,
  compares each category against its budget, flags overspending, surfaces
  likely duplicate charges, and shows the trend versus the prior period.
  This is where small problems get caught while they are still small, so
  the flags must be reliable and explainable. Percent-of-budget is rounded
  to the nearest whole percent. Amounts are in a single currency (USD).

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: A category is over budget only when spending exceeds its budget

    Scenario: A category over its budget is flagged
      Given the Dining budget is 600 dollars this month
      And the household has spent 850 dollars on Dining this month
      When the advisor generates a spending report for this month
      Then the report flags Dining as over budget
      And the report shows Dining at 142 percent of budget

    Scenario: A category exactly at its budget is not flagged
      Given the Groceries budget is 900 dollars this month
      And the household has spent 900 dollars on Groceries this month
      When the advisor generates a spending report for this month
      Then the report does not flag Groceries as over budget

    Scenario: A category within its budget is not flagged
      Given the Groceries budget is 900 dollars this month
      And the household has spent 720 dollars on Groceries this month
      When the advisor generates a spending report for this month
      Then the report does not flag Groceries as over budget

    Scenario: Every category that exceeds its budget is flagged
      Given the Dining budget is 600 dollars this month
      And the household has spent 850 dollars on Dining this month
      And the Shopping budget is 400 dollars this month
      And the household has spent 500 dollars on Shopping this month
      When the advisor generates a spending report for this month
      Then the report flags Dining as over budget
      And the report flags Shopping as over budget

  Rule: Spending in a category with no budget is reported without a budget comparison

    Scenario: An unbudgeted category is reported but not flagged as over budget
      Given no budget is set for Travel this month
      And the household has spent 300 dollars on Travel this month
      When the advisor generates a spending report for this month
      Then the report shows 300 dollars spent on Travel
      And the report does not flag Travel as over budget

  Rule: Likely duplicate charges are surfaced for review

    Scenario: Two identical charges on the same day are flagged as a possible duplicate
      Given a charge of 49.99 dollars from "Acme Streaming" on the 14th
      And another charge of 49.99 dollars from "Acme Streaming" on the 14th
      When the advisor generates a spending report for this month
      Then the report flags a possible duplicate charge from "Acme Streaming"

    Scenario: Two charges from the same merchant for different amounts are not flagged as duplicates
      Given a charge of 49.99 dollars from "Acme Streaming" on the 14th
      And another charge of 9.99 dollars from "Acme Streaming" on the 14th
      When the advisor generates a spending report for this month
      Then the report does not flag a possible duplicate charge from "Acme Streaming"

  Rule: Spending is compared to the prior period

    Scenario: The report shows the change versus the prior month
      Given the household spent 4000 dollars last month
      And the household has spent 4600 dollars this month
      When the advisor generates a spending report for this month
      Then the report shows spending up 600 dollars versus the prior month

  Rule: An empty period is reported as no spending, not an error

    Scenario: A month with no transactions reports zero spending
      Given the household has no transactions this month
      When the advisor generates a spending report for this month
      Then the report shows 0 dollars spent
      And the report flags no categories as over budget

  Rule: An expired session is reported as needing re-authentication

    Scenario: An expired session asks the household to re-authenticate
      Given the household's Monarch session has expired
      When the advisor generates a spending report for this month
      Then the advisor reports that re-authentication is required
