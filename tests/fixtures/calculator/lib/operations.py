"""Arithmetic operations module."""


def add(a, b):
    """Add two numbers.

    Args:
        a: First operand (int or float)
        b: Second operand (int or float)

    Returns:
        Sum of a and b
    """
    return a + b


def subtract(a, b):
    """Subtract b from a.

    Args:
        a: First operand (int or float)
        b: Second operand (int or float)

    Returns:
        Difference of a and b
    """
    return a - b


def multiply(a, b):
    """Multiply two numbers.

    Args:
        a: First operand (int or float)
        b: Second operand (int or float)

    Returns:
        Product of a and b
    """
    return a * b


def divide(a, b):
    """Divide a by b.

    Args:
        a: Dividend (int or float)
        b: Divisor (int or float)

    Returns:
        Quotient of a divided by b

    Raises:
        ValueError: If b is zero
    """
    if b == 0:
        raise ValueError("Division by zero is not allowed")
    return a / b
