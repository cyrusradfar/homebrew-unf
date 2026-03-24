#!/usr/bin/env python3
"""
Generate UNF* branded app icons for the UNFUDGED desktop app.

Usage:
    python3 scripts/generate-icons.py

This script generates:
  - Master 1024x1024 PNG with full "UNF*" branding
  - Resized PNG variants (256x256, 128x128, 32x32)
  - macOS .icns file via iconutil
  - Windows .ico file

Color scheme:
  - Background: #0a0a0a (near-black)
  - Text "UNF": #fafafa (white)
  - Asterisk "*": #f59e0b (amber)
  - Rounded corners at ~22% radius

To customize, modify the COLORS and FONT_* constants below.
"""

import os
import subprocess
import sys
from pathlib import Path

try:
    from PIL import Image, ImageDraw, ImageFont
except ImportError:
    print("Error: Pillow (PIL) is required. Install it with:")
    print("  pip3 install Pillow")
    sys.exit(1)


# ============================================================================
# Configuration
# ============================================================================

COLORS = {
    "background": "#0a0a0a",
    "text": "#fafafa",
    "asterisk": "#f59e0b",
}

FONT_FALLBACK_CHAIN = [
    "/System/Library/Fonts/SFMono-Bold.otf",
    "/System/Library/Fonts/Menlo.ttc",
    "/Library/Fonts/Courier New Bold.ttf",
]

# Icon output paths
ICONS_DIR = Path(__file__).parent.parent / "app" / "icons"
MASTER_ICON = ICONS_DIR / "1024x1024.png"

ICON_SPECS = [
    # (filename, size, is_asterisk_only)
    ("128x128@2x.png", 256, False),
    ("128x128.png", 128, False),
    ("32x32.png", 32, True),
]

MACOS_ICNS_SIZES = [
    (16, 1),
    (16, 2),
    (32, 1),
    (32, 2),
    (128, 1),
    (128, 2),
    (256, 1),
    (256, 2),
    (512, 1),
    (512, 2),
]


# ============================================================================
# Utility Functions
# ============================================================================

def find_font(size=128):
    """
    Find a bold monospace font, trying the fallback chain.
    Falls back to default font with a warning if nothing is found.
    """
    for font_path in FONT_FALLBACK_CHAIN:
        if os.path.exists(font_path):
            try:
                return ImageFont.truetype(font_path, size)
            except Exception as e:
                print(f"Warning: Failed to load {font_path}: {e}")
                continue

    print("Warning: No suitable font found. Using Pillow default font.")
    return ImageFont.load_default()


def create_rounded_rectangle(image, xy, radius=50, fill=None, outline=None, width=1):
    """
    Draw a rounded rectangle on the image.

    Args:
        image: PIL Image object
        xy: Bounding box (left, top, right, bottom)
        radius: Corner radius in pixels
        fill: Fill color
        outline: Outline color
        width: Outline width
    """
    draw = ImageDraw.Draw(image)
    x1, y1, x2, y2 = xy

    # Draw four corners (arcs) and four sides (lines)
    draw.arc((x1, y1, x1 + 2*radius, y1 + 2*radius), 180, 270, fill=outline, width=width)
    draw.arc((x2 - 2*radius, y1, x2, y1 + 2*radius), 270, 360, fill=outline, width=width)
    draw.arc((x2 - 2*radius, y2 - 2*radius, x2, y2), 0, 90, fill=outline, width=width)
    draw.arc((x1, y2 - 2*radius, x1 + 2*radius, y2), 90, 180, fill=outline, width=width)

    # Fill rectangle with interior
    if fill:
        draw.rectangle((x1 + radius, y1, x2 - radius, y2), fill=fill)
        draw.rectangle((x1, y1 + radius, x2, y2 - radius), fill=fill)
        draw.polygon([
            (x1 + radius, y1),
            (x2 - radius, y1),
            (x2 - radius, y1 + radius),
            (x2, y1 + radius),
            (x2, y2 - radius),
            (x2 - radius, y2 - radius),
            (x2 - radius, y2),
            (x1 + radius, y2),
            (x1 + radius, y2 - radius),
            (x1, y2 - radius),
            (x1, y1 + radius),
            (x1 + radius, y1 + radius),
        ], fill=fill)


def generate_icon(size, asterisk_only=False):
    """
    Generate a single icon at the given size.

    Args:
        size: Icon size in pixels (e.g., 1024, 256, 128, 32)
        asterisk_only: If True, render only the asterisk (for small icons)

    Returns:
        PIL Image object
    """
    # Create base image
    image = Image.new("RGBA", (size, size), COLORS["background"])
    draw = ImageDraw.Draw(image)

    # Draw rounded background (22% radius)
    radius = int(size * 0.22)
    create_rounded_rectangle(
        image,
        (0, 0, size - 1, size - 1),
        radius=radius,
        fill=COLORS["background"]
    )

    if asterisk_only:
        # For 32x32: just render the asterisk, nearly filling the icon
        font_size = int(size * 0.7)
        font = find_font(font_size)
        text = "*"
        bbox = draw.textbbox((0, 0), text, font=font)
        text_width = bbox[2] - bbox[0]
        text_height = bbox[3] - bbox[1]
        x = (size - text_width) // 2
        y = (size - text_height) // 2
        draw.text((x, y), text, fill=COLORS["asterisk"], font=font)
    else:
        # For larger icons: render "UNF" + "*"
        padding = int(size * 0.2)
        available_width = size - 2 * padding
        available_height = size - 2 * padding

        # Estimate font size to fit within available space
        # Start with a reasonable proportion of height
        font_size = int(available_height * 0.5)
        font = find_font(font_size)

        # Render "UNF*" with asterisk in different color
        text_unf = "UNF"
        text_ast = "*"

        # Get bounding boxes to calculate positioning
        bbox_unf = draw.textbbox((0, 0), text_unf, font=font)
        bbox_ast = draw.textbbox((0, 0), text_ast, font=font)

        text_unf_width = bbox_unf[2] - bbox_unf[0]
        text_ast_width = bbox_ast[2] - bbox_ast[0]
        total_width = text_unf_width + text_ast_width

        text_height = bbox_unf[3] - bbox_unf[1]

        # Center horizontally and vertically
        start_x = (size - total_width) // 2
        start_y = (size - text_height) // 2

        # Draw "UNF" in white
        draw.text((start_x, start_y), text_unf, fill=COLORS["text"], font=font)

        # Draw "*" in amber, positioned right after "UNF"
        ast_x = start_x + text_unf_width
        draw.text((ast_x, start_y), text_ast, fill=COLORS["asterisk"], font=font)

    return image


def generate_png_variants():
    """Generate PNG icon variants."""
    print("Generating PNG variants...")

    # Generate master 1024x1024
    master = generate_icon(1024, asterisk_only=False)
    master.save(MASTER_ICON, "PNG")
    print(f"  ✓ {MASTER_ICON} ({master.size[0]}x{master.size[1]})")

    # Generate resized variants
    for filename, size, asterisk_only in ICON_SPECS:
        if asterisk_only:
            icon = generate_icon(size, asterisk_only=True)
        else:
            icon = generate_icon(size, asterisk_only=False)

        output_path = ICONS_DIR / filename
        icon.save(output_path, "PNG")
        print(f"  ✓ {output_path} ({icon.size[0]}x{icon.size[1]})")


def generate_icns():
    """Generate macOS .icns file using iconutil."""
    print("Generating macOS .icns...")

    # Create temporary iconset directory
    iconset_dir = ICONS_DIR / "icon.iconset"
    iconset_dir.mkdir(parents=True, exist_ok=True)

    # Generate all required sizes for the iconset
    for base_size, scale in MACOS_ICNS_SIZES:
        pixel_size = base_size * scale
        size_label = f"{base_size}x{base_size}" if scale == 1 else f"{base_size}x{base_size}@{scale}x"

        icon = generate_icon(pixel_size, asterisk_only=False)
        icon_filename = f"icon_{size_label}.png"
        icon_path = iconset_dir / icon_filename
        icon.save(icon_path, "PNG")

    print(f"  ✓ Created iconset directory: {iconset_dir}")

    # Convert iconset to .icns using iconutil
    output_icns = ICONS_DIR / "icon.icns"
    try:
        subprocess.run(
            ["iconutil", "-c", "icns", "-o", str(output_icns), str(iconset_dir)],
            check=True,
            capture_output=True,
        )
        print(f"  ✓ {output_icns}")
    except FileNotFoundError:
        print("  ⚠ iconutil not found. Skipping .icns generation.")
        print("    (This is normal on non-macOS systems.)")
    except subprocess.CalledProcessError as e:
        print(f"  ⚠ iconutil failed: {e.stderr.decode().strip()}")


def generate_ico():
    """Generate Windows .ico file."""
    print("Generating Windows .ico...")

    # Collect sizes for the .ico file
    ico_sizes = [
        (256, False),  # 256x256 (from master or generate)
        (128, False),  # 128x128
        (64, False),   # 64x64
        (32, True),    # 32x32 asterisk-only
        (16, True),    # 16x16 asterisk-only
    ]

    images = []
    for size, asterisk_only in ico_sizes:
        icon = generate_icon(size, asterisk_only=asterisk_only)
        images.append(icon)

    output_ico = ICONS_DIR / "icon.ico"
    images[0].save(
        output_ico,
        "ICO",
        sizes=[(img.width, img.height) for img in images]
    )
    print(f"  ✓ {output_ico}")


# ============================================================================
# Main
# ============================================================================

def main():
    """Main entry point."""
    print("=" * 70)
    print("UNF* Icon Generator")
    print("=" * 70)

    # Ensure icons directory exists
    ICONS_DIR.mkdir(parents=True, exist_ok=True)
    print(f"\nOutput directory: {ICONS_DIR}\n")

    # Generate icons
    generate_png_variants()
    print()
    generate_icns()
    print()
    generate_ico()

    print("\n" + "=" * 70)
    print("Icon generation complete!")
    print("=" * 70)


if __name__ == "__main__":
    main()
