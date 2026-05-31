@ISSUE-22
Feature: progress_vs_goals graceful handling of missing goals configuration
  The progress_vs_goals tool helps families track financial goals.
  When no goals file has been configured yet, first-run users must
  receive a helpful response rather than a server crash. Permission
  errors (a misconfigured path) are still surfaced as real errors.

  Background:
    Given the budgeting advisor is connected to the household's finances

  Rule: A missing goals file is not an error — it means no goals are configured yet

    Scenario: No goals file returns a helpful guidance response
      Given the MCP server is running with no goals file configured
      When an agent calls the progress_vs_goals tool
      Then the response indicates no goals are configured
      And the response includes guidance on how to add goals
      And the response is not an MCP error

    Scenario: Empty goals file returns the same helpful response
      Given the MCP server is running with an empty goals file
      When an agent calls the progress_vs_goals tool
      Then the response indicates no goals are configured
      And the response is not an MCP error

  Rule: A goals file with a permission error is still a real error

    Scenario: Unreadable goals file returns an MCP error
      Given the MCP server is running with a goals file that cannot be read due to permissions
      When an agent calls the progress_vs_goals tool
      Then the response is an MCP error
      And the error message mentions the goals file path
