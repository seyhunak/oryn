@api @US-5 @US-6
Feature: Products API
  The catalogue API (requirements US-5, US-6). The search scenario is
  affected by an intermittent 503 from the search backend (planted defect #2),
  so expect it to fail roughly one run in four while DEMO_FLAKY=1.

  Scenario: Service health
    When I send a GET request to "/api/health"
    Then the response status should be 200
    And the response JSON "$.status" should equal "ok"

  Scenario: List the whole catalogue
    When I send a GET request to "/api/products"
    Then the response status should be 200
    And the response JSON "$.count" should equal "8"
    And the response JSON "$.products" should have length 8
    And the response JSON "$.products[0].id" should exist
    And the response JSON "$.products[0].name" should exist
    And the response JSON "$.products[0].price" should exist
    And the response JSON "$.products[0].sku" should exist
    And the response time should be below 500 ms

  Scenario: Fetch a single product
    When I send a GET request to "/api/products/claw-hammer"
    Then the response status should be 200
    And the response JSON "$.id" should equal "claw-hammer"
    And the response JSON "$.name" should equal "Claw Hammer"
    And the response JSON "$.price" should equal "12.5"
    And the response JSON "$.category" should equal "Hand tools"

  Scenario: Unknown product returns a JSON 404
    When I send a GET request to "/api/products/flux-capacitor"
    Then the response status should be 404
    And the response JSON "$.error.code" should equal "not_found"
    And the response JSON "$.error.message" should exist

  Scenario: Search the catalogue
    When I send a GET request to "/api/products?q=drill"
    Then the response status should be 200
    And the response JSON "$.count" should equal "1"
    And the response JSON "$.products[0].id" should equal "cordless-drill"
    And the response time should be below 500 ms

  Scenario: The OpenAPI document is published
    When I send a GET request to "/openapi.json"
    Then the response status should be 200
    And the response JSON "$.openapi" should equal "3.0.3"
    And the response JSON "$.info.title" should equal "Northwind Supply API"
