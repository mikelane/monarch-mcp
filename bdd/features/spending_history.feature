@ISSUE-54
Feature: Spending history
  Multi-month true-spending baseline. Aggregates transaction data into
  per-month summaries with a fixed-vs-discretionary split. Raw transactions
  are never returned — the output is compact aggregates only. Income and
  transfers are excluded by the same rules as spending_report.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: Each month in the range is reported with its true spending total

    Scenario: Two months with spending are each reported separately
      Given the household spent 850 dollars on Dining in 2026-03
      And the household spent 920 dollars on Groceries in 2026-04
      When the advisor generates spending history for 2026-03-01 to 2026-04-30
      Then the history contains 2 monthly entries
      And the month 2026-03 shows total spending of 850 dollars
      And the month 2026-04 shows total spending of 920 dollars

    Scenario: A month with no transactions reports zero spending
      Given the household has no transactions in the history range
      When the advisor generates spending history for 2026-03-01 to 2026-03-31
      Then the history contains 1 monthly entries
      And the month 2026-03 shows total spending of 0 dollars

  Rule: Income and transfers are excluded from all monthly totals

    Scenario: Income transactions do not contribute to monthly spending
      Given the household received 5000 dollars of Paychecks income in 2026-03
      And the household spent 400 dollars on Groceries in 2026-03
      When the advisor generates spending history for 2026-03-01 to 2026-03-31
      Then the month 2026-03 shows total spending of 400 dollars

    Scenario: Transfer transactions do not contribute to monthly spending
      Given the household made a 2000 dollar Credit Card Payment transfer in 2026-03
      And the household spent 400 dollars on Groceries in 2026-03
      When the advisor generates spending history for 2026-03-01 to 2026-03-31
      Then the month 2026-03 shows total spending of 400 dollars

  Rule: Spending is split into fixed and discretionary buckets

    Scenario: Mortgage is classified as fixed; dining is discretionary
      Given the household spent 2500 dollars on Mortgage in 2026-03
      And the household spent 300 dollars on Dining in 2026-03
      When the advisor generates spending history for 2026-03-01 to 2026-03-31
      Then the month 2026-03 fixed spending is 2500 dollars
      And the month 2026-03 discretionary spending is 300 dollars

    Scenario: Fixed plus discretionary equals the total
      Given the household spent 2500 dollars on Mortgage in 2026-03
      And the household spent 150 dollars on Utilities in 2026-03
      And the household spent 300 dollars on Dining in 2026-03
      When the advisor generates spending history for 2026-03-01 to 2026-03-31
      Then the month 2026-03 fixed spending is 2650 dollars
      And the month 2026-03 discretionary spending is 300 dollars
      And the month 2026-03 total spending equals fixed plus discretionary

    Scenario: A discretionary category whose name contains a fixed substring stays discretionary
      # "Concert Rentals" contains the substring "rent"; "Accidental Purchases"
      # contains "dental". Whole-word tokenization (ADR 0011) must NOT misclassify
      # either as a fixed obligation. This pins the substring-false-positive fix at
      # the MCP boundary, not just in the pure Rust predicate.
      Given the household spent 300 dollars on Concert Rentals in 2026-03
      And the household spent 120 dollars on Accidental Purchases in 2026-03
      And the household spent 2500 dollars on Mortgage in 2026-03
      When the advisor generates spending history for 2026-03-01 to 2026-03-31
      Then the month 2026-03 fixed spending is 2500 dollars
      And the month 2026-03 discretionary spending is 420 dollars
      And the month 2026-03 total spending equals fixed plus discretionary

  Rule: The output is compact aggregates — no raw transaction list

    Scenario: The response contains per-month aggregates, not individual transactions
      Given the household spent 850 dollars on Dining in 2026-03
      And the household spent 365 dollars on Groceries in 2026-03
      When the advisor generates spending history for 2026-03-01 to 2026-03-31
      Then the response does not contain a raw transaction list
      And the month 2026-03 by-category shows Dining at 850 dollars
      And the month 2026-03 by-category shows Groceries at 365 dollars

  Rule: An expired session is reported as needing re-authentication

    Scenario: An expired session asks the household to re-authenticate
      Given the household's Monarch session has expired
      When the advisor generates spending history for 2026-03-01 to 2026-03-31
      Then the advisor reports that re-authentication is required
