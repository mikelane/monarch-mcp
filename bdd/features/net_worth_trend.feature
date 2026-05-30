@ISSUE-B2
Feature: Net worth trend
  A single net-worth number is a snapshot; a trend tells the story. This tool
  returns net worth across a date range, shows how each account type contributed
  to the movement, identifies the biggest single mover, and splits the picture
  into assets versus liabilities so the household can tell growth from debt
  paydown.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: The trend reports net worth month-by-month across the requested period

    Scenario: Three months of snapshots produce three data points
      Given the household's net worth by account type over the past 3 months is:
        | month   | account_type | balance  |
        | 2026-03 | depository   |  10000   |
        | 2026-03 | brokerage    |  50000   |
        | 2026-03 | credit       |  -5000   |
        | 2026-04 | depository   |  11000   |
        | 2026-04 | brokerage    |  52000   |
        | 2026-04 | credit       |  -4800   |
        | 2026-05 | depository   |  12000   |
        | 2026-05 | brokerage    |  54000   |
        | 2026-05 | credit       |  -4600   |
      When the advisor requests a net worth trend for the past 3 months
      Then the trend contains 3 monthly data points
      And the trend reports a net worth of 61400 dollars in the most recent month
      And the trend reports a net worth change of positive 6400 dollars over the period

    Scenario: Single month with no prior data yields one data point and zero change
      Given the household's net worth by account type over the past 1 month is:
        | month   | account_type | balance |
        | 2026-05 | depository   | 20000   |
      When the advisor requests a net worth trend for the past 1 month
      Then the trend contains 1 monthly data point
      And the trend reports a net worth change of 0 dollars over the period

  Rule: The trend breaks down movement by account type

    Scenario: Biggest mover is the account type with the largest absolute balance change
      Given the household's net worth by account type over the past 2 months is:
        | month   | account_type | balance  |
        | 2026-04 | depository   |  10000   |
        | 2026-04 | brokerage    |  40000   |
        | 2026-04 | credit       |  -3000   |
        | 2026-05 | depository   |  10200   |
        | 2026-05 | brokerage    |  45000   |
        | 2026-05 | credit       |  -2900   |
      When the advisor requests a net worth trend for the past 2 months
      Then the trend identifies brokerage as the biggest mover
      And the trend reports brokerage moved by positive 5000 dollars

    Scenario: Liability reduction is reported as a positive contribution to net worth
      Given the household's net worth by account type over the past 2 months is:
        | month   | account_type | balance  |
        | 2026-04 | credit       | -10000   |
        | 2026-05 | credit       |  -7000   |
      When the advisor requests a net worth trend for the past 2 months
      Then the trend reports credit moved by positive 3000 dollars

  Rule: The trend splits the view into total assets and total liabilities

    Scenario: Asset and liability totals are reported separately for the latest month
      Given the household's net worth by account type over the past 1 month is:
        | month   | account_type | balance  |
        | 2026-05 | depository   |  15000   |
        | 2026-05 | brokerage    |  60000   |
        | 2026-05 | credit       |  -8000   |
        | 2026-05 | loan         | -20000   |
      When the advisor requests a net worth trend for the past 1 month
      Then the trend reports total assets of 75000 dollars
      And the trend reports total liabilities of 28000 dollars

  Rule: No snapshot data yields zeros, not an error

    Scenario: Empty snapshot history returns zeros and no error
      Given the household has no net worth snapshot history
      When the advisor requests a net worth trend for the past 3 months
      Then the trend contains 0 monthly data points
      And the trend reports a net worth of 0 dollars in the most recent month

  Rule: An expired session surfaces a re-authentication prompt

    Scenario: Expired session blocks the trend
      Given the household's Monarch session has expired
      When the advisor requests a net worth trend for the past 3 months
      Then the advisor reports that re-authentication is required
