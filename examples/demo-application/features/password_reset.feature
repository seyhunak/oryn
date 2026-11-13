@browser @US-4
Feature: Password reset
  A customer who forgot their password can request a reset link
  (requirements US-4). The confirmation text must match AC-4.2 exactly.

  Scenario: Request a reset link from the login page
    Given I open "/login"
    When I click "Forgot your password?"
    Then the URL should contain "/forgot-password"
    When I fill "Email" with "demo@northwind.test"
    And I click "Send reset link"
    Then I should see "If an account exists, a reset link has been sent."
    And I take a screenshot "reset-requested"

  Scenario: Unknown email gets the same confirmation
    Given I open "/forgot-password"
    When I fill "Email" with "nobody@example.test"
    And I click "Send reset link"
    Then I should see "If an account exists, a reset link has been sent."

  Scenario: An invalid reset token is refused
    Given I open "/reset-password?token=not-a-real-token"
    Then I should see "This reset link is invalid or has expired."
