@ISSUE-67
Feature: asset_allocation splits net worth by asset class
  The household needs a view of their portfolio by asset class — equities,
  cash, real estate, crypto, other assets, and liabilities — with each class
  showing its dollar total and its percentage of gross assets.

  This is the asset-class lens: orthogonal to account_inventory (tax treatment).
  The same accounts, a different grouping axis.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: Accounts are grouped into asset classes with correct percentages

    Scenario: Brokerage, checking, and property split into three classes
      Given the following accounts for asset allocation:
        | name         | type        | subtype   | balance   |
        | My Brokerage | brokerage   | brokerage | 78000.00  |
        | Checking     | depository  | checking  | 12000.00  |
        | My Property  | real_estate | house     | 10000.00  |
      When the advisor requests asset_allocation
      Then the "equities" class total is 78000.00
      And the "equities" class percent_of_assets is 78.00
      And the "cash" class total is 12000.00
      And the "cash" class percent_of_assets is 12.00
      And the "real_estate" class total is 10000.00
      And the "real_estate" class percent_of_assets is 10.00

  Rule: Liabilities reduce net worth but are excluded from the asset-class percentage base

    Scenario: Liabilities reported separately from the asset mix
      Given the following accounts for asset allocation:
        | name        | type       | subtype     | balance    |
        | Checking    | depository | checking    | 100000.00  |
        | Credit Card | credit     | credit_card | -5000.00   |
      When the advisor requests asset_allocation
      Then the allocation total_liabilities is -5000.00
      And the allocation net_worth is 95000.00
      And the allocation gross_assets is 100000.00
      And the "liabilities" class has no percent_of_assets

  Rule: An unrecognized account subtype is bucketed as other and flagged

    Scenario: Unknown account type lands in other class and is flagged
      Given the following accounts for asset allocation:
        | name          | type        | subtype | balance |
        | Mystery Asset | collectible | art     | 4000.00 |
      When the advisor requests asset_allocation
      Then the "other" class total is 4000.00
      And the "other" class is flagged as not recognized

  Rule: The net worth rollup agrees with the signed sum of all account balances

    Scenario: Net worth equals the signed sum of all account balances
      Given the following accounts for asset allocation:
        | name         | type        | subtype     | balance    |
        | Roth IRA     | brokerage   | roth        | 100000.00  |
        | Checking     | depository  | checking    | 20000.00   |
        | Credit Card  | credit      | credit_card | -3000.00   |
        | Mortgage     | loan        | other       | -200000.00 |
      When the advisor requests asset_allocation
      Then the allocation net_worth is -83000.00

  Rule: Hidden accounts are included in the totals

    Scenario: Hidden account contributes to its class total
      Given the following accounts for asset allocation:
        | name        | type      | subtype | balance   | is_hidden |
        | Hidden IRA  | brokerage | roth    | 50000.00  | true      |
        | Checking    | depository| checking| 10000.00  | false     |
      When the advisor requests asset_allocation
      Then the "equities" class total is 50000.00
      And the "cash" class total is 10000.00
      And the allocation gross_assets is 60000.00

  Rule: An expired session surfaces a re-authentication prompt

    Scenario: Expired session blocks asset allocation
      Given the household's Monarch session has expired
      When the advisor requests asset_allocation
      Then the advisor reports that re-authentication is required
