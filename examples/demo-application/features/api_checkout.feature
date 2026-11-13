@api @US-2 @US-7 @US-8
Feature: Checkout API
  Authenticated customers can fill a cart and check out over the API
  (requirements US-2, US-7, US-8).

  Background:
    Given I send a POST request to "/api/__reset"
    And I send a POST request to "/api/auth/login" with body:
      """
      {"email": "demo@northwind.test", "password": "demo-password"}
      """
    And the response status should be 200
    And the response JSON "$.user.email" should equal "demo@northwind.test"
    And I store the response JSON "$.token" as "token"
    And I set header "Authorization" to "Bearer {{token}}"

  Scenario: Wrong credentials are rejected
    When I send a POST request to "/api/auth/login" with body:
      """
      {"email": "demo@northwind.test", "password": "wrong"}
      """
    Then the response status should be 401
    And the response JSON "$.error.code" should equal "invalid_credentials"

  Scenario: An invalid token cannot read the cart
    Given I set header "Authorization" to "Bearer not-a-valid-token"
    When I send a GET request to "/api/cart"
    Then the response status should be 401

  Scenario Outline: Quantity <quantity> is rejected
    When I send a POST request to "/api/cart/items" with body:
      """
      {"product_id": "claw-hammer", "quantity": <quantity>}
      """
    Then the response status should be 400
    And the response JSON "$.error.code" should equal "invalid_quantity"

    Examples:
      | quantity |
      | 0        |
      | 100      |

  Scenario: Single item order pays flat shipping
    When I send a POST request to "/api/cart/items" with body:
      """
      {"product_id": "cordless-drill", "quantity": 1}
      """
    Then the response status should be 201
    When I send a POST request to "/api/checkout"
    Then the response status should be 201
    And the response JSON "$.subtotal" should equal "89"
    And the response JSON "$.tax" should equal "7.12"
    And the response JSON "$.shipping" should equal "7.5"
    And the response JSON "$.total" should equal "103.62"
    And the response JSON "$.currency" should equal "USD"

  Scenario: Two of a cheap item are priced per the spec
    When I send a POST request to "/api/cart/items" with body:
      """
      {"product_id": "claw-hammer", "quantity": 2}
      """
    Then the response status should be 201
    And the response JSON "$.items" should have length 1
    And the response JSON "$.subtotal" should equal "25"
    When I send a POST request to "/api/checkout" with body:
      """
      {"shipping_name": "Demo Customer", "shipping_address": "1 Demo Street, Springfield"}
      """
    Then the response status should be 201
    And I store the response JSON "$.order_id" as "order_id"
    When I send a GET request to "/api/orders/{{order_id}}"
    Then the response status should be 200
    And the response JSON "$.order_id" should equal "{{order_id}}"
    And the response JSON "$.subtotal" should equal "25"
    And the response JSON "$.shipping" should equal "7.5"
    And the response JSON "$.tax" should equal "2"
    And the response JSON "$.total" should equal "34.5"

  Scenario: Checking out an empty cart fails
    When I send a POST request to "/api/checkout"
    Then the response status should be 400
    And the response JSON "$.error.code" should equal "empty_cart"
