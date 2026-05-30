@ISSUE-A4
Feature: Financial overview snapshot
  The advisor's situational-awareness tool. In one call it answers
  "where does the household stand right now?" — combining every account's
  balance into a net-worth figure, this month's cash flow, and how net
  worth has moved since last month. Every advising session starts here,
  so the rollup must be correct before any guidance is trustworthy.
  All amounts are in a single currency (USD); multi-currency is out of scope.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: Net worth is assets minus liabilities across all accounts

    Scenario: Net worth combines asset and liability accounts
      Given the household holds 100000 dollars across asset accounts
      And the household owes 30000 dollars across liability accounts
      When the advisor requests a financial overview
      Then the overview reports a net worth of 70000 dollars

    Scenario: Net worth is negative when liabilities exceed assets
      Given the household holds 10000 dollars across asset accounts
      And the household owes 30000 dollars across liability accounts
      When the advisor requests a financial overview
      Then the overview reports a net worth of negative 20000 dollars

  Rule: The overview reports this month's cash flow

    Scenario: Cash flow separates income, spending, and net
      Given this month the household received 8000 dollars of income
      And this month the household spent 6500 dollars
      When the advisor requests a financial overview
      Then the overview reports income of 8000 dollars
      And the overview reports spending of 6500 dollars
      And the overview reports net cash flow of 1500 dollars

  Rule: The overview surfaces month-over-month net-worth movement

    Scenario: Net worth shows the value and the change since last month
      Given the household's net worth was 68000 dollars last month
      And the household's net worth is 70000 dollars this month
      When the advisor requests a financial overview
      Then the overview reports a net worth of 70000 dollars
      And the overview reports a net-worth change of positive 2000 dollars

  Rule: An empty financial picture is reported as zeros, not an error

    Scenario: No accounts and no activity yields a zero overview
      Given the household has no accounts
      When the advisor requests a financial overview
      Then the overview reports a net worth of 0 dollars
      And the overview reports net cash flow of 0 dollars

  Rule: An expired session is reported as needing re-authentication

    Scenario: An expired session asks the household to re-authenticate
      Given the household's Monarch session has expired
      When the advisor requests a financial overview
      Then the advisor reports that re-authentication is required
