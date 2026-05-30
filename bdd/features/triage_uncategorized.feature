@ISSUE-A6 @not_implemented
Feature: Triage and tidy uncategorized transactions
  Trustworthy data is the foundation of every other tool, so the advisor
  keeps the books honest. Triage finds transactions needing review and
  proposes a category from the household's own history, but applies
  nothing on its own. A separate apply step commits only the changes the
  household approved, and it can only recategorize, tag, or annotate —
  it can never create, delete, or move money.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: Uncategorized transactions get category suggestions from history

    Scenario: A known merchant is suggested its historical category
      Given past transactions from "Blue Bottle" were categorized as Coffee
      And a new uncategorized transaction from "Blue Bottle"
      When the advisor triages uncategorized transactions
      Then the proposed change categorizes the "Blue Bottle" transaction as Coffee

  Rule: Triage proposes changes without applying them

    Scenario: Triage leaves the transactions unchanged
      Given an uncategorized transaction from "Blue Bottle"
      When the advisor triages uncategorized transactions
      Then the "Blue Bottle" transaction remains uncategorized

  Rule: Applying a changeset commits only the approved changes

    Scenario: Applying a changeset recategorizes exactly the approved transactions
      Given a proposed change categorizing the "Blue Bottle" transaction as Coffee
      When the advisor applies the approved changeset
      Then the "Blue Bottle" transaction is categorized as Coffee

    Scenario: Applying a changeset never changes the number of transactions
      Given the month has 40 transactions
      And a proposed change categorizing one transaction as Coffee
      When the advisor applies the approved changeset
      Then the month still has 40 transactions
