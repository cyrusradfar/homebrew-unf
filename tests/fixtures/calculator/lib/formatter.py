"""Result formatting module."""


def format_result(result):
    """Format a calculation result for display.

    Integers are displayed without decimal points.
    Floats are displayed with reasonable precision (up to 10 decimal places,
    with trailing zeros removed).

    Args:
        result: A numeric result to format

    Returns:
        Formatted string representation of the result
    """
    # Check if result is effectively an integer
    if isinstance(result, int) or (isinstance(result, float) and result.is_integer()):
        return str(int(result))

    # For floats, format with reasonable precision and remove trailing zeros
    formatted = f"{result:.10f}".rstrip('0').rstrip('.')
    return formatted
