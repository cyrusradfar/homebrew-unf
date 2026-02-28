#!/usr/bin/env python3
"""Calculator CLI entry point."""

import sys
import re
from lib.operations import add, subtract, multiply, divide
from lib.formatter import format_result


def parse_expression(expression):
    """Parse a simple arithmetic expression.

    Supports operators: +, -, *, /
    Expression format: "operand operator operand" (with flexible spacing)

    Args:
        expression: String like "2 + 3" or "10/2"

    Returns:
        Tuple of (operand1, operator, operand2)

    Raises:
        ValueError: If expression cannot be parsed
    """
    # Match pattern: number, operator, number (with flexible spacing)
    pattern = r'^\s*(-?\d+\.?\d*)\s*([+\-*/])\s*(-?\d+\.?\d*)\s*$'
    match = re.match(pattern, expression)

    if not match:
        raise ValueError(f"Invalid expression format: {expression}")

    operand1_str, operator, operand2_str = match.groups()

    # Convert strings to numbers (int or float)
    operand1 = float(operand1_str) if '.' in operand1_str else int(operand1_str)
    operand2 = float(operand2_str) if '.' in operand2_str else int(operand2_str)

    return operand1, operator, operand2


def calculate(operand1, operator, operand2):
    """Perform the calculation based on the operator.

    Args:
        operand1: First operand
        operator: One of '+', '-', '*', '/'
        operand2: Second operand

    Returns:
        Calculated result

    Raises:
        ValueError: If operator is unknown or invalid (e.g., division by zero)
    """
    if operator == '+':
        return add(operand1, operand2)
    elif operator == '-':
        return subtract(operand1, operand2)
    elif operator == '*':
        return multiply(operand1, operand2)
    elif operator == '/':
        return divide(operand1, operand2)
    else:
        raise ValueError(f"Unknown operator: {operator}")


def main():
    """Main entry point for the calculator CLI."""
    if len(sys.argv) != 2:
        print("Usage: python main.py '<expression>'", file=sys.stderr)
        print("Example: python main.py '2 + 3'", file=sys.stderr)
        sys.exit(1)

    expression = sys.argv[1]

    try:
        operand1, operator, operand2 = parse_expression(expression)
        result = calculate(operand1, operator, operand2)
        formatted = format_result(result)
        print(formatted)
    except ValueError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"Unexpected error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == '__main__':
    main()
