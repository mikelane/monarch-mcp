Feature: retirement_readiness checks withdrawal coverage against baseline spend
  # @ISSUE-69
  # The retirement_readiness tool combines the investable portfolio (Equities-class
  # accounts from asset_allocation) with the annualised baseline spend (from
  # compute_true_spending) into a safe-withdrawal-rate coverage check.
  #
  # Assumptions surfaced in every response:
  #   - withdrawal_rate_used: the rate applied (default 0.04)
  #   - spend_window_months: how many trailing months fed the spend baseline
  #   - invested_assets_note: explains what counts as "invested"

  Background:
    Given the budgeting advisor is connected to the household's finances

  # Intentionally extreme fixture: 1 M portfolio vs 480 k/yr spend → coverage ≈ 0.083.
  # This exercises the low-coverage path (well under 1×) to ensure the tool
  # reports a meaningful gap rather than a near-breakeven result.
  @ISSUE-69
  Scenario: Coverage ratio at the default 4 percent rule
    Given the following accounts for retirement readiness:
      | name        | type      | subtype   | balance   |
      | 401k        | brokerage | st_401k   | 600000.0  |
      | Roth IRA    | brokerage | roth      | 400000.0  |
      | Checking    | depository| checking  | 50000.0   |
    And 6 months of expense transactions totalling 240000.0
    When the advisor requests retirement_readiness
    Then the retirement_readiness invested_assets is 1000000.0
    And the retirement_readiness annual_baseline_spend is 480000.0
    And the retirement_readiness sustainable_annual_withdrawal is 40000.0
    And the retirement_readiness coverage_ratio is 0.083333
    And the retirement_readiness target_portfolio is 12000000.0
    And the retirement_readiness withdrawal_rate_used is 0.04

  @ISSUE-69
  Scenario: A spending level above sustainable withdrawals reports a gap
    Given the following accounts for retirement readiness:
      | name     | type      | subtype   | balance  |
      | 401k     | brokerage | st_401k   | 500000.0 |
      | Checking | depository| checking  | 30000.0  |
    And 6 months of expense transactions totalling 120000.0
    When the advisor requests retirement_readiness
    Then the retirement_readiness invested_assets is 500000.0
    And the retirement_readiness coverage_ratio is less than 1.0
    And the retirement_readiness surplus_or_gap is negative

  @ISSUE-69
  Scenario: A custom withdrawal rate is honored and echoed
    Given the following accounts for retirement readiness:
      | name     | type      | subtype   | balance  |
      | Roth IRA | brokerage | roth      | 800000.0 |
      | Checking | depository| checking  | 20000.0  |
    And 6 months of expense transactions totalling 120000.0
    When the advisor requests retirement_readiness with withdrawal_rate 0.035
    Then the retirement_readiness withdrawal_rate_used is 0.035

  @ISSUE-69
  Scenario: An out-of-range withdrawal rate is rejected with a clear message
    Given the following accounts for retirement readiness:
      | name     | type      | subtype   | balance  |
      | 401k     | brokerage | st_401k   | 500000.0 |
    And 6 months of expense transactions totalling 60000.0
    When the advisor requests retirement_readiness with withdrawal_rate 0.5
    Then the retirement_readiness response contains an error about withdrawal rate range

  @ISSUE-69
  Scenario: Primary residence and vehicles are excluded from invested assets
    Given the following accounts for retirement readiness:
      | name          | type        | subtype   | balance   |
      | Brokerage     | brokerage   | brokerage | 800000.0  |
      | Primary Home  | real_estate | house     | 400000.0  |
      | Car           | vehicle     | car       | 25000.0   |
    And 6 months of expense transactions totalling 120000.0
    When the advisor requests retirement_readiness
    Then the retirement_readiness invested_assets is 800000.0

  @ISSUE-69
  Scenario: Session expired surfaces re-auth message
    Given the household's Monarch session has expired
    When the advisor requests retirement_readiness
    Then the advisor reports that re-authentication is required
