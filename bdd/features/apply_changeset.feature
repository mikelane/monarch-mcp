@ISSUE-53
Feature: apply_changeset resolves category names to UUIDs
  The apply_changeset tool accepts category names at the MCP boundary
  (e.g. "Pets") but Monarch's API requires a category UUID. The tool must
  resolve each name to its UUID before sending the mutation. Unknown names
  must be rejected — they are never sent to the API.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: A known category name is resolved to its UUID before the mutation

    Scenario: A known category name is sent to Monarch as its UUID
      Given a transaction "txn-1" exists with category "Uncategorized"
      And the category "Pets" has UUID "cat-uuid-pets"
      When the advisor applies a changeset setting transaction "txn-1" category to "Pets"
      Then the mutation for transaction "txn-1" recorded categoryId "cat-uuid-pets"
      And no rejected changes are reported

  Rule: An unknown category name is rejected and never sent to the API

    Scenario: An unknown category name is rejected with a clear reason
      Given a transaction "txn-2" exists with category "Uncategorized"
      And no category named "NoSuchCategory" exists
      When the advisor applies a changeset setting transaction "txn-2" category to "NoSuchCategory"
      Then no mutation is recorded for transaction "txn-2"
      And a rejected change is reported for transaction "txn-2" mentioning "NoSuchCategory"

  Rule: Tags and notes changes are unaffected by category resolution

    Scenario: A tags-only change passes through without category lookup interference
      Given a transaction "txn-3" exists with category "Coffee"
      And the category "Coffee" has UUID "cat-uuid-coffee"
      When the advisor applies a changeset adding tag "important" to transaction "txn-3"
      Then the mutation for transaction "txn-3" recorded no categoryId
      And no rejected changes are reported

  Rule: An ambiguous category name (multiple UUIDs sharing one name) is rejected

    Scenario: An ambiguous category name is rejected without sending to the API
      Given a transaction "txn-4" exists with category "Uncategorized"
      And two categories named "Pets" exist with UUIDs "pet-uuid-system" and "pet-uuid-custom"
      When the advisor applies a changeset setting transaction "txn-4" category to "Pets"
      Then no mutation is recorded for transaction "txn-4"
      And a rejected change is reported for transaction "txn-4" mentioning "ambiguous"
