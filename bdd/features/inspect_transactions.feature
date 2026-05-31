@ISSUE-23
Feature: Inspect transactions for a category or merchant
  The advisor needs to diagnose spending anomalies — a $12k Pets spike or a
  $3.3k Medical charge — and sometimes needs to re-categorize transactions
  that are already categorized (not just the "needs review" ones). Neither
  task was possible before this tool because no tool listed categorized
  transactions with their ids.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: Drill-down returns line items with ids

    Scenario: Inspecting a category returns all transactions with their ids
      Given three Pets transactions this month with known ids
      When the advisor inspects transactions in the "Pets" category
      Then the result contains all three Pets transactions
      And every returned transaction has a non-empty id

    Scenario: Inspecting with no filter returns all transactions for the month
      Given five transactions this month across different categories
      When the advisor inspects transactions with no filters
      Then the result contains all five transactions

  Rule: Compound summary includes inflow vs outflow split

    Scenario: A category with a charge and a refund splits into inflow and outflow
      Given a Pets charge of $12000 and a Petco refund of $500 this month
      When the advisor inspects transactions in the "Pets" category
      Then the summary shows total_outflow of $12000
      And the summary shows total_inflow of $500
      And the summary shows net_amount of $-11500

  Rule: Merchant filter narrows the result

    Scenario: Filtering by merchant returns only matching transactions
      Given a "Petco" transaction and a "Banfield Vet" transaction both in Pets
      When the advisor inspects transactions for merchant "Petco"
      Then the result contains only the "Petco" transaction

  Rule: An inspected id is accepted by apply_changeset

    Scenario: A transaction id from inspect_transactions is valid for apply_changeset
      Given a Pets transaction with id "txn-inspect-42" this month
      When the advisor inspects transactions in the "Pets" category
      And the advisor applies a changeset recategorizing "txn-inspect-42" as "Veterinary"
      Then the apply result reports the change was applied without rejection

  Rule: inspect_transactions is read-only

    Scenario: Calling inspect_transactions does not modify any transaction
      Given three Pets transactions this month with known ids
      When the advisor inspects transactions in the "Pets" category
      Then no transaction was modified in the mock

  Rule: An empty filter string is transparent — identical to omitting the filter

    Scenario: An empty-string category filter returns the same transactions as no filter
      Given five transactions this month across different categories
      When the advisor inspects transactions with category filter ""
      Then the result contains all five transactions

  Rule: An expired session is reported as needing re-authentication

    Scenario: An expired session asks the household to re-authenticate
      Given the household's Monarch session has expired
      When the advisor inspects transactions in the "Pets" category
      Then the advisor reports that re-authentication is required
