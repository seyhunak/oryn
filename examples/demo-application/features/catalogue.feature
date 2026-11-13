@browser @US-5 @US-6
Feature: Catalogue
  Shoppers can browse and search the product catalogue (requirements US-5, US-6).

  Scenario: Browse all products
    Given I open "/products"
    Then the page title should be "Products | Northwind Supply"
    And I should see "Claw Hammer"
    And I should see "Aluminium Step Ladder"
    And "Add Claw Hammer to cart" should be visible

  Scenario: Open a product detail page
    Given I open "/products"
    When I click "Cordless Drill 18V"
    Then the URL should contain "/products/cordless-drill"
    And I should see "SKU NW-2001"
    And "Quantity" should be visible
    And "css=[data-testid=price]" should contain "$89.00"

  Scenario: Unknown product shows a not found page
    Given I open "/products/flux-capacitor"
    Then I should see "Page not found"

  Scenario Outline: Search the catalogue for "<term>"
    Given I open "/products"
    When I fill "Search products" with "<term>"
    And I press "Enter"
    Then the URL should contain "q="
    And I should see "<expected>"
    And I should not see "<absent>"

    Examples:
      | term        | expected                | absent             |
      | hammer      | Claw Hammer             | Cordless Drill 18V |
      | SAFETY      | Safety Glasses          | Claw Hammer        |
      | NW-2001     | Cordless Drill 18V      | Tape Measure 25 ft |
      | screwdriver | No products found.      | Claw Hammer        |
