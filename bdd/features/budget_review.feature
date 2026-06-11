@ISSUE-66
Feature: budget_review shows mid-month budget pacing per category
  The advisor needs to know which expense categories are burning through their
  budget faster than the month is progressing. budget_review computes a
  straight-line pace fraction from the current day and compares each category's
  percent_spent against that fraction (with a ±10pp tolerance).

  MONARCH_NOW is pinned to 2026-05-15 (day 15 of 31) in environment.py.
  Pace fraction = 15/31 ≈ 48.4%. Tolerance band: [38.4%, 58.4%].

  Background:
    Given the budgeting advisor is connected to the household's finances

  Scenario: A category spent faster than the month has elapsed is flagged over-pace
    Given a "Restaurants" budget of 400 dollars
    And 360 dollars spent on "Restaurants" so far this month
    When the advisor requests budget_review
    Then "Restaurants" shows budget 400.0 and spent 360.0
    And "Restaurants" remaining is 40.0
    And "Restaurants" pace_status is "over"

  Scenario: A category tracking with the calendar is on track
    Given a "Groceries" budget of 600 dollars
    And 290 dollars spent on "Groceries" so far this month
    When the advisor requests budget_review
    Then "Groceries" pace_status is "on_track"

  Scenario: A category already past its budget is flagged over_budget
    Given a "Phone" budget of 200 dollars
    And 210 dollars spent on "Phone" so far this month
    When the advisor requests budget_review
    Then "Phone" pace_status is "over_budget"
    And "Phone" remaining is -10.0

  Scenario: Income and transfer categories are not paced
    Given a "Restaurants" budget of 400 dollars
    And 360 dollars spent on "Restaurants" so far this month
    And an income transaction of 5000 dollars in category "Paychecks"
    And a transfer transaction of 2000 dollars in category "Credit Card Payment"
    When the advisor requests budget_review
    Then "Paychecks" does not appear in the pacing list
    And "Credit Card Payment" does not appear in the pacing list
    And "Restaurants" appears in the pacing list

  Scenario: The rollup counts reflect pacing status across categories
    Given a "Restaurants" budget of 400 dollars
    And 360 dollars spent on "Restaurants" so far this month
    And a "Groceries" budget of 600 dollars
    And 290 dollars spent on "Groceries" so far this month
    When the advisor requests budget_review
    Then the rollup shows at least 1 over category
    And the rollup shows at least 1 on_track category

  Scenario: The response does not contain a raw transaction list
    Given a "Groceries" budget of 600 dollars
    And 200 dollars spent on "Groceries" so far this month
    When the advisor requests budget_review
    Then the budget_review response does not contain raw transactions

  Scenario: An expired session asks the household to re-authenticate
    Given the household's Monarch session has expired
    When the advisor requests budget_review
    Then the advisor reports that re-authentication is required
