@ISSUE-B3
Feature: Recurring charge scan
  Hidden subscription creep is money lost invisibly. This tool scans the
  household's recurring charges and surfaces two signals: charges whose amount
  has drifted from the stream's expected value ("creeping" charges), and
  upcoming renewals due in the current period. Stable subscriptions — same
  amount, on schedule — are reported but not flagged, so the signal stays
  meaningful.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: A recurring charge is flagged as creeping when its amount differs from the stream's expected amount

    Scenario: A subscription that increased in price is flagged as creeping
      Given the household has the following recurring charges this period:
        | merchant     | stream_amount | actual_amount | frequency | is_approximate |
        | StreamingCo  |         9.99  |        13.99  | monthly   | false          |
        | GymMembership|        50.00  |        50.00  | monthly   | false          |
      When the advisor runs a recurring charge scan
      Then the scan flags StreamingCo as a creeping charge
      And the scan reports StreamingCo increased by 4.00 dollars
      And the scan does not flag GymMembership as creeping

    Scenario: A charge with an approximate amount is not flagged as creeping
      Given the household has the following recurring charges this period:
        | merchant     | stream_amount | actual_amount | frequency | is_approximate |
        | ElectricUtil |       120.00  |       134.50  | monthly   | true           |
      When the advisor runs a recurring charge scan
      Then the scan does not flag ElectricUtil as creeping

    Scenario: A charge that decreased in price is also flagged as a change worth noting
      Given the household has the following recurring charges this period:
        | merchant     | stream_amount | actual_amount | frequency | is_approximate |
        | MusicService |        10.99  |         7.99  | monthly   | false          |
      When the advisor runs a recurring charge scan
      Then the scan flags MusicService as a creeping charge
      And the scan reports MusicService changed by 3.00 dollars

  Rule: Upcoming renewals are listed separately from past charges

    Scenario: Future recurring items appear in the upcoming renewals list
      Given the household has the following recurring charges this period:
        | merchant     | stream_amount | is_past | frequency |
        | RentPayment  |      1500.00  | true    | monthly   |
        | AnnualBackup |        99.00  | false   | annually  |
        | StreamingCo  |        13.99  | false   | monthly   |
      When the advisor runs a recurring charge scan
      Then the upcoming renewals list contains AnnualBackup
      And the upcoming renewals list contains StreamingCo
      And the upcoming renewals list does not contain RentPayment

  Rule: No recurring charges yields an empty scan, not an error

    Scenario: Household with no recurring charges returns an empty scan
      Given the household has no recurring charges this period
      When the advisor runs a recurring charge scan
      Then the scan returns no creeping charges
      And the scan returns no upcoming renewals

  Rule: A stable subscription does not pollute the creeping-charge list

    Scenario: A charge equal to the stream amount is not flagged
      Given the household has the following recurring charges this period:
        | merchant     | stream_amount | actual_amount | frequency | is_approximate |
        | CloudStorage |         2.99  |         2.99  | monthly   | false          |
        | NewsFeed     |        14.99  |        14.99  | monthly   | false          |
      When the advisor runs a recurring charge scan
      Then the scan does not flag CloudStorage as creeping
      And the scan does not flag NewsFeed as creeping
      And the scan returns no creeping charges

  Rule: An expired session surfaces a re-authentication prompt

    Scenario: Expired session blocks the scan
      Given the household's Monarch session has expired
      When the advisor runs a recurring charge scan
      Then the advisor reports that re-authentication is required
