@ISSUE-50
Feature: Account inventory — retirement-planning buckets
  The household needs a clear view of how their accounts map to retirement
  planning categories: tax-advantaged (401k, Roth, HSA), taxable brokerage,
  cash (depository), other assets (vehicles), and liabilities. This tool
  groups accounts into those buckets and surfaces a net-worth rollup for
  cross-checking with financial_overview.

  The tool is read-only: it never modifies any account.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: Brokerage accounts are split by subtype into tax-advantaged or taxable

    Scenario: 401k account appears in the tax_advantaged bucket
      Given the household has the following accounts:
        | name          | type      | subtype  | balance   |
        | My 401k       | brokerage | st_401k  | 50000.00  |
      When the advisor runs an account inventory
      Then the inventory has a "tax_advantaged" bucket
      And the "tax_advantaged" bucket contains 1 account
      And the "tax_advantaged" bucket total is 50000.00 dollars

    Scenario: Roth IRA account appears in the tax_advantaged bucket
      Given the household has the following accounts:
        | name          | type      | subtype  | balance    |
        | Roth IRA      | brokerage | roth     | 200000.00  |
      When the advisor runs an account inventory
      Then the inventory has a "tax_advantaged" bucket
      And the "tax_advantaged" bucket contains 1 account

    Scenario: HSA account appears in the tax_advantaged bucket
      Given the household has the following accounts:
        | name          | type      | subtype                | balance  |
        | HSA           | brokerage | health_savings_account | 5000.00  |
      When the advisor runs an account inventory
      Then the inventory has a "tax_advantaged" bucket

    Scenario: Taxable brokerage account appears in the taxable_brokerage bucket
      Given the household has the following accounts:
        | name          | type      | subtype    | balance   |
        | Individual    | brokerage | brokerage  | 10000.00  |
      When the advisor runs an account inventory
      Then the inventory has a "taxable_brokerage" bucket
      And the inventory does not have a "tax_advantaged" bucket

    Scenario: Stock plan account appears in the taxable_brokerage bucket
      Given the household has the following accounts:
        | name          | type      | subtype     | balance  |
        | RSU Grant     | brokerage | stock_plan  | 0.00     |
      When the advisor runs an account inventory
      Then the inventory has a "taxable_brokerage" bucket

  Rule: Depository accounts go into the cash bucket

    Scenario: Checking account appears in the cash bucket
      Given the household has the following accounts:
        | name          | type       | subtype  | balance  |
        | Checking      | depository | checking | 5000.00  |
      When the advisor runs an account inventory
      Then the inventory has a "cash" bucket
      And the "cash" bucket total is 5000.00 dollars

    Scenario: Savings account appears in the cash bucket
      Given the household has the following accounts:
        | name          | type       | subtype  | balance   |
        | Emergency Fund| depository | savings  | 20000.00  |
      When the advisor runs an account inventory
      Then the inventory has a "cash" bucket

  Rule: Credit and loan accounts go into the liabilities bucket

    Scenario: Credit card appears in the liabilities bucket with negative balance
      Given the household has the following accounts:
        | name          | type   | subtype     | balance    |
        | Visa          | credit | credit_card | -3000.00   |
      When the advisor runs an account inventory
      Then the inventory has a "liabilities" bucket
      And the "liabilities" bucket total is -3000.00 dollars

    Scenario: Mortgage appears in the liabilities bucket
      Given the household has the following accounts:
        | name          | type | subtype | balance      |
        | Mortgage      | loan | other   | -200000.00   |
      When the advisor runs an account inventory
      Then the inventory has a "liabilities" bucket

  Rule: Vehicle accounts go into the other_assets bucket

    Scenario: Car appears in the other_assets bucket
      Given the household has the following accounts:
        | name          | type    | subtype | balance   |
        | My Car        | vehicle | car     | 15000.00  |
      When the advisor runs an account inventory
      Then the inventory has a "other_assets" bucket
      And the "other_assets" bucket total is 15000.00 dollars

  Rule: Net-worth rollup reconciles assets minus liabilities

    Scenario: Rollup net worth equals total assets minus total liabilities
      Given the household has the following accounts:
        | name          | type       | subtype  | balance     |
        | Checking      | depository | checking | 10000.00    |
        | Roth IRA      | brokerage  | roth     | 200000.00   |
        | Credit Card   | credit     | credit_card | -5000.00 |
        | Mortgage      | loan       | other    | -100000.00  |
      When the advisor runs an account inventory
      Then the inventory rollup shows total assets of 210000.00 dollars
      And the inventory rollup shows total liabilities of 105000.00 dollars
      And the inventory rollup shows net worth of 105000.00 dollars

  Rule: Hidden accounts are included but flagged

    Scenario: Hidden account is present in results with is_hidden flag
      Given the household has the following accounts:
        | name          | type       | subtype  | balance  | is_hidden |
        | Old Savings   | depository | savings  | 100.00   | true      |
      When the advisor runs an account inventory
      Then the inventory has a "cash" bucket
      And the "cash" bucket contains 1 account
      And the account "Old Savings" in the "cash" bucket is flagged as hidden

  Rule: Unknown subtypes are flagged but still bucketed

    Scenario: Unrecognized brokerage subtype is bucketed as taxable and flagged
      Given the household has the following accounts:
        | name          | type      | subtype        | balance  |
        | Future Fund   | brokerage | future_product | 8000.00  |
      When the advisor runs an account inventory
      Then the inventory has a "taxable_brokerage" bucket
      And the account "Future Fund" in the "taxable_brokerage" bucket is flagged as unknown subtype

  Rule: Accounts with null balance are flagged as balance unknown

    @ISSUE-50
    Scenario: Unsynced account with null currentBalance is flagged balance_unknown
      Given the household has the following accounts:
        | name            | type       | subtype | balance |
        | Unsynced Saving | depository | savings | null    |
      When the advisor runs an account inventory
      Then the inventory has a "cash" bucket
      And the account "Unsynced Saving" in the "cash" bucket is flagged as balance unknown

  Rule: Accounts with null subtype fall back to the type-level bucket

    @ISSUE-50
    Scenario: Brokerage account with null subtype falls back to taxable_brokerage bucket
      Given the household has the following accounts:
        | name         | type      | subtype | balance   |
        | Mystery Fund | brokerage | null    | 15000.00  |
      When the advisor runs an account inventory
      Then the inventory has a "taxable_brokerage" bucket
      And the "taxable_brokerage" bucket contains 1 account

  Rule: An expired session surfaces a re-authentication prompt

    Scenario: Expired session blocks the inventory
      Given the household's Monarch session has expired
      When the advisor runs an account inventory
      Then the advisor reports that re-authentication is required
