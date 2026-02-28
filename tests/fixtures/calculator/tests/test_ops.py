"""Unit tests for arithmetic operations."""

import pytest
from lib.operations import add, subtract, multiply, divide
from lib.formatter import format_result


class TestAdd:
    """Tests for addition operation."""

    def test_add_positive_integers(self):
        """Test adding two positive integers."""
        assert add(2, 3) == 5

    def test_add_negative_integers(self):
        """Test adding negative integers."""
        assert add(-2, -3) == -5

    def test_add_mixed_sign(self):
        """Test adding integers with mixed signs."""
        assert add(5, -3) == 2

    def test_add_floats(self):
        """Test adding floating point numbers."""
        assert add(1.5, 2.5) == 4.0

    def test_add_zero(self):
        """Test adding with zero."""
        assert add(5, 0) == 5


class TestSubtract:
    """Tests for subtraction operation."""

    def test_subtract_positive_integers(self):
        """Test subtracting positive integers."""
        assert subtract(5, 3) == 2

    def test_subtract_negative_integers(self):
        """Test subtracting negative integers."""
        assert subtract(-2, -5) == 3

    def test_subtract_mixed_sign(self):
        """Test subtracting with mixed signs."""
        assert subtract(5, -3) == 8

    def test_subtract_floats(self):
        """Test subtracting floating point numbers."""
        assert subtract(5.5, 2.5) == 3.0

    def test_subtract_zero(self):
        """Test subtracting zero."""
        assert subtract(5, 0) == 5


class TestMultiply:
    """Tests for multiplication operation."""

    def test_multiply_positive_integers(self):
        """Test multiplying positive integers."""
        assert multiply(3, 4) == 12

    def test_multiply_negative_integers(self):
        """Test multiplying negative integers."""
        assert multiply(-3, -4) == 12

    def test_multiply_mixed_sign(self):
        """Test multiplying with mixed signs."""
        assert multiply(3, -4) == -12

    def test_multiply_floats(self):
        """Test multiplying floating point numbers."""
        assert multiply(2.5, 4.0) == 10.0

    def test_multiply_by_zero(self):
        """Test multiplying by zero."""
        assert multiply(5, 0) == 0


class TestDivide:
    """Tests for division operation."""

    def test_divide_positive_integers(self):
        """Test dividing positive integers."""
        assert divide(10, 2) == 5.0

    def test_divide_negative_integers(self):
        """Test dividing negative integers."""
        assert divide(-10, -2) == 5.0

    def test_divide_mixed_sign(self):
        """Test dividing with mixed signs."""
        assert divide(10, -2) == -5.0

    def test_divide_floats(self):
        """Test dividing floating point numbers."""
        assert divide(10.0, 4.0) == 2.5

    def test_divide_by_zero(self):
        """Test that dividing by zero raises ValueError."""
        with pytest.raises(ValueError, match="Division by zero"):
            divide(10, 0)

    def test_divide_zero_by_number(self):
        """Test dividing zero by a number."""
        assert divide(0, 5) == 0.0


class TestFormatter:
    """Tests for result formatting."""

    def test_format_integer(self):
        """Test formatting an integer result."""
        assert format_result(5) == "5"

    def test_format_whole_float(self):
        """Test formatting a float that is a whole number."""
        assert format_result(5.0) == "5"

    def test_format_float_with_decimals(self):
        """Test formatting a float with decimal places."""
        result = format_result(2.5)
        assert result == "2.5"

    def test_format_float_precision(self):
        """Test formatting maintains reasonable precision."""
        result = format_result(1/3)
        assert result.startswith("0.333333")

    def test_format_negative_integer(self):
        """Test formatting negative integer."""
        assert format_result(-5) == "-5"

    def test_format_negative_float(self):
        """Test formatting negative float."""
        assert format_result(-2.5) == "-2.5"
