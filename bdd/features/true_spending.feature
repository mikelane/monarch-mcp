@ISSUE-25
Feature: True spending excludes inter-account transfers and surfaces refund pairs
  "Spending" means money leaving the family, not money moving between
  their own accounts. Credit card payments and transfers are inter-account
  movements and must not inflate the spending figure. Charge-then-refund
  pairs should be surfaced so the advisor can explain anomalies rather
  than hiding or double-counting them.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: Transfer and Credit Card Payment transactions do not count toward spending

    Scenario: A Transfer transaction is excluded from total_spent
      Given a mock Monarch month containing a Transfer transaction of -3299.02
      And an expense transaction in Groceries of -731.27
      When an agent calls spending_report for that month
      Then total_spent reflects only the Groceries amount

    Scenario: A Credit Card Payment transaction is excluded from total_spent
      Given a mock Monarch month containing a Credit Card Payment transaction of -4364.48
      And an expense transaction in Restaurants of -1054.09
      When an agent calls spending_report for that month
      Then total_spent reflects only the Restaurants amount

    Scenario: A Transfer transaction is excluded from financial_overview spending
      Given a mock Monarch month containing a Transfer transaction of -3299.02
      And an expense transaction in Groceries of -731.27
      When an agent calls financial_overview for that month
      Then the financial overview spending reflects only the Groceries amount

  Rule: financial_overview and spending_report agree on total spending

    Scenario: Both tools report the same spending for the same transactions
      Given an expense transaction in Groceries of -731.27
      And an expense transaction in Restaurants of -200.00
      When an agent calls spending_report for that month
      And an agent also calls financial_overview for that month
      Then the financial overview spending equals the spending report total

  Rule: Charge-and-refund pairs from the same merchant are reported as possible reversals

    Scenario: A charge and same-merchant refund within the month appear as a reversal pair
      Given a mock Monarch month where merchant "Banfield" has a charge of -850.00 and a refund of 850.00
      When an agent calls spending_report for that month
      Then the response includes a possible_reversals entry for merchant "Banfield"
      And the possible_reversals entry identifies both the charge and the refund

    Scenario: Two same-amount charges without a refund are not paired as a reversal
      Given a mock Monarch month where merchant "Banfield" has two charges of -850.00 each
      When an agent calls spending_report for that month
      Then the response has no possible_reversals entry for merchant "Banfield"
