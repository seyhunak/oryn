@browser @US-8
Feature: Cart and checkout
  Totals at checkout follow the pricing rules in requirements US-8:
  8% tax on the subtotal, $7.50 shipping below $100, free at $100 or more.

  Background:
    Given I open "/login"
    And I fill "Email" with "demo@northwind.test"
    And I fill "Password" with "demo-password"
    And I click "Sign in"
    And I should see "Welcome, Demo Customer"

  Scenario: Buy two of a cheap item
    Given I open "/products/claw-hammer"
    When I fill "Quantity" with "2"
    And I click "Add to cart"
    Then I should see "Added Claw Hammer to your cart."
    And I should see "Subtotal: $25.00"
    When I click "Checkout"
    Then the URL should contain "/checkout"
    When I fill "Full name" with "Demo Customer"
    And I fill "Shipping address" with "1 Demo Street, Springfield"
    And I click "Place order"
    Then I should see "Order confirmed"
    And I take a screenshot "order-confirmation"
    And "css=[data-testid=order-total]" should contain "Total: $34.50"

  Scenario: A single item under $100 pays flat shipping
    Given I open "/products/cordless-drill"
    When I click "Add to cart"
    And I click "Checkout"
    And I fill "Shipping address" with "1 Demo Street, Springfield"
    And I click "Place order"
    Then I should see "Order confirmed"
    And I should see "Total: $103.62"

  Scenario: Remove an item from the cart
    Given I open "/products"
    When I click "Add Safety Glasses to cart"
    Then I should see "Safety Glasses"
    When I click "Remove Safety Glasses"
    Then I should see "Your cart is empty."
