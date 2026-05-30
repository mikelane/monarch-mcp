@ISSUE-A5 @not_implemented
Feature: Spending report
  The advisor's weekly/monthly workhorse. It groups spending by category,
  compares each category against its budget, flags overspending, surfaces
  likely duplicate charges, and shows the trend versus the prior period.
  This is where small problems get caught while they are still small, so
  the flags must be reliable and explainable.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: Spending is compared against budget per category

    Scenario: A category over its budget is flagged
      Given the Dining budget is 600 dollars this month
      And the household has spent 850 dollars on Dining this month
      When the advisor generates a spending report for this month
      Then the report flags Dining as over budget
      And the report shows Dining at 142 percent of budget

    Scenario: A category within its budget is not flagged
      Given the Groceries budget is 900 dollars this month
      And the household has spent 720 dollars on Groceries this month
      When the advisor generates a spending report for this month
      Then the report does not flag Groceries as over budget

  Rule: Likely duplicate charges are surfaced for review

    Scenario: Two identical charges on the same day are flagged as a possible duplicate
      Given a charge of 49.99 dollars from "Acme Streaming" on the 14th
      And another charge of 49.99 dollars from "Acme Streaming" on the 14th
      When the advisor generates a spending report for this month
      Then the report flags a possible duplicate charge from "Acme Streaming"

  Rule: Spending is compared to the prior period

    Scenario: The report shows the change versus the prior month
      Given the household spent 4000 dollars last month
      And the household has spent 4600 dollars this month
      When the advisor generates a spending report for this month
      Then the report shows spending up 600 dollars versus the prior month
