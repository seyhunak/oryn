@browser @US-1 @US-2 @US-3
Feature: Authentication
  Customers can create an account, sign in, and are told when their
  credentials are wrong (requirements US-1, US-2, US-3).

  Note: registration uses a fixed email address; reset the demo state with
  POST /api/__reset (or restart the app) before re-running this feature.

  Scenario: Register a new account
    Given I open "/register"
    When I fill "Name" with "Ada Lovelace"
    And I fill "Email" with "ada@example.test"
    And I fill "Password" with "analytical-engine"
    And I click "Create account"
    Then the URL should contain "/dashboard"
    And I should see "Welcome, Ada Lovelace"

  Scenario: Log in with the demo account
    Given I open "/login"
    When I fill "Email" with "demo@northwind.test"
    And I fill "Password" with "demo-password"
    And I click "Sign in"
    Then the URL should contain "/dashboard"
    And I should see "Welcome, Demo Customer"
    And I should see "Recent orders"
    And the page title should be "Dashboard | Northwind Supply"

  Scenario: Wrong password is rejected
    Given I open "/login"
    When I fill "Email" with "demo@northwind.test"
    And I fill "Password" with "not-the-password"
    And I click "Sign in"
    Then I should see "Invalid email or password"
    And the URL should contain "/login"
    But I should not see "Welcome, Demo Customer"

  Scenario: Dashboard requires login
    Given I open "/dashboard"
    Then the URL should contain "/login"
    And "Sign in" should be visible
