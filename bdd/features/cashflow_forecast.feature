@ISSUE-B1
Feature: Cash flow forecast
  The household needs to know whether they will end the month in the black
  before it happens, not after. This tool combines the current account
  balances, this-period income and spending so far, and scheduled recurring
  charges to produce a projected month-end position — and a clear shortfall
  flag when outflows are on track to exceed available funds.

  The forecast is read-only and advisory: it never moves money or changes
  any transaction.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: Projected month-end = current balance + remaining income − remaining bills

    Scenario: Positive projection when income covers all upcoming bills
      Given the household has a checking account with a balance of 3000 dollars
      And this month 4000 dollars of income has already been received
      And the following recurring bills are still due this month:
        | merchant        | amount |
        | Rent            | 1500   |
        | Electric        |  120   |
        | Internet        |   80   |
      When the advisor runs a cash flow forecast
      Then the forecast shows a projected month-end balance of 1300 dollars
      And the forecast does not flag a shortfall

    Scenario: Shortfall flagged when upcoming bills exceed available funds
      Given the household has a checking account with a balance of 500 dollars
      And this month 0 dollars of income has already been received
      And the following recurring bills are still due this month:
        | merchant        | amount |
        | Rent            | 1500   |
        | Subscription    |   15   |
      When the advisor runs a cash flow forecast
      Then the forecast flags a projected shortfall of 1015 dollars
      And the forecast names the bills driving the shortfall

    Scenario: No upcoming bills leaves balance unchanged in forecast
      Given the household has a checking account with a balance of 2000 dollars
      And this month 2000 dollars of income has already been received
      And there are no recurring bills due this month
      When the advisor runs a cash flow forecast
      Then the forecast shows a projected month-end balance of 2000 dollars
      And the forecast does not flag a shortfall

  Rule: Past recurring items in the current period are excluded from the projection

    Scenario: Bills already paid this month do not reduce the projected balance again
      Given the household has a checking account with a balance of 1800 dollars
      And the following recurring bills were already paid this month:
        | merchant     | amount |
        | Rent         | 1200   |
      And the following recurring bills are still due this month:
        | merchant     | amount |
        | Electric     |  100   |
      When the advisor runs a cash flow forecast
      Then the forecast shows a projected month-end balance of 1700 dollars
      And the forecast does not flag a shortfall

  Rule: An expired session surfaces a re-authentication prompt

    Scenario: Expired session blocks the forecast
      Given the household's Monarch session has expired
      When the advisor runs a cash flow forecast
      Then the advisor reports that re-authentication is required
