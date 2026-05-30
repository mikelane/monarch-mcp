@ISSUE-A6
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

    Scenario: An unfamiliar merchant gets no category suggestion
      Given no past transactions from "Mystery Merchant"
      And a new uncategorized transaction from "Mystery Merchant"
      When the advisor triages uncategorized transactions
      Then no category is proposed for the "Mystery Merchant" transaction

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

  Rule: The tidy path can only change category, tags, and notes

    Scenario: Applying a changeset preserves every transaction id
      Given the month has 40 transactions
      And a proposed change categorizing one transaction as Coffee
      When the advisor applies the approved changeset
      Then the month still has the same 40 transactions
      And only category, tag, and note fields were changed

    Scenario: A changeset entry that tries to alter an amount is rejected
      Given a proposed change that sets a transaction amount to 0 dollars
      When the advisor applies the approved changeset
      Then no transaction amount is changed
      And the advisor reports the disallowed change was rejected

  Rule: An expired session is reported as needing re-authentication

    Scenario: An expired session asks the household to re-authenticate
      Given the household's Monarch session has expired
      When the advisor triages uncategorized transactions
      Then the advisor reports that re-authentication is required
