# Design System Specification: Atmospheric Luminescence

## 1. Overview & Creative North Star

**The Creative North Star: "The Ethereal Curator"**

This design system rejects the clinical flatness of traditional web interfaces in favor of a visionOS-inspired, multidimensional experience. It is defined by "Atmospheric Depth"—a methodology where UI elements are not merely placed on a screen, but suspended within a luminous, pressurized environment.

By leveraging high translucency, pearlescent gradients, and the brutalist-leaning precision of **Space Grotesk**, we move beyond simple "Light Mode." We are creating a high-end editorial experience that feels architectural, intentional, and weightless. The layout breaks the traditional grid through staggered layering and intentional asymmetry, ensuring the interface feels like a bespoke digital gallery rather than a template.

---

## 2. Colors & Surface Philosophy

The palette is anchored in a crisp, surgical white (`#f5f7f9`) but energized by the "Electric Cyan" primary accent.

### The "No-Line" Rule

**Explicit Instruction:** Designers are prohibited from using 1px solid borders for sectioning or containment. Boundaries must be defined exclusively through:

1. **Background Color Shifts:** Placing a `surface-container-low` section against a `surface` background.
2. **Tonal Transitions:** Using the hierarchy of `surface-container` tiers (Lowest to Highest) to denote nested importance.
3. **Backdrop Blurs:** Utilizing `24px+` blurs to create separation through optical depth.

### Surface Hierarchy & Nesting

Treat the UI as a physical stack of frosted glass sheets.

- **Base Layer:** `surface` (#f5f7f9) - The infinite canvas.
- **Secondary Sections:** `surface-container-low` (#eef1f3) - For subtle content grouping.
- **Floating Components:** `surface-container-lowest` (#ffffff) with 80% opacity and `backdrop-filter: blur(24px)`.
- **Interactive Overlays:** `primary-container` (#00cffc) - Reserved for high-signal moments.

### The "Glass & Gradient" Rule

Standard flat fills are insufficient. Main CTAs and Hero backgrounds must utilize a linear gradient:

- **Signature Gradient:** `primary` (#00647b) to `primary-container` (#00cffc) at a 135-degree angle. This provides the "visual soul" required for a premium feel.

---

## 3. Typography: The Editorial Voice

We pair the technical, geometric precision of **Space Grotesk** with the approachable clarity of **Manrope**.

- **Display & Headlines (Space Grotesk):** Used to command authority. The high x-height and technical terminals of Space Grotesk should be tracked tightly (-2%) in `display-lg` to create a dense, editorial impact.
- **Body & Utility (Manrope):** Manrope provides a humanistic counter-balance. Its neutral structure ensures readability in dense data environments while the `space-grotesk` labels maintain the brand’s "Electric" DNA.
- **Hierarchy as Identity:** Use extreme scale contrast. A `display-lg` (3.5rem) headline paired immediately with a `body-sm` (0.75rem) caption creates a sophisticated, high-fashion layout rhythm.

---

## 4. Elevation & Depth: Tonal Layering

Traditional drop shadows are replaced by **Ambient Luminance**.

### The Layering Principle

Depth is achieved by "stacking" the surface tiers. To lift a card, do not reach for a shadow; instead, place a `surface-container-lowest` (#ffffff) element on top of a `surface-container` (#e5e9eb) background.

### Ambient Shadows

When an element must "float" (e.g., a modal or floating action button):

- **Blur:** 40px to 64px.
- **Opacity:** 4% - 8%.
- **Color:** Use a tinted version of `on-surface` (#2c2f31) or a soft `primary` tint to mimic the way light refracts through glass.

### The "Ghost Border" Fallback

If accessibility requirements demand a container boundary, use a **Ghost Border**:

- **Token:** `outline-variant` (#abadaf) at **15% opacity**.
- **Strict Rule:** 100% opaque borders are strictly forbidden.

---

## 5. Components

### Buttons

- **Primary:** Gradient fill (`primary` to `primary-container`), white text (`on-primary`), `md` (1.5rem) corner radius. No shadow.
- **Secondary:** Glassmorphic base (`surface-container-lowest` @ 50% opacity), `24px` backdrop blur, `Ghost Border`.
- **Tertiary:** Text-only in `primary` (#00647b), using `Space Grotesk` Label-md weight.

### Input Fields

- **Base:** `surface-container-low` fill.
- **Active State:** Transition to `surface-container-lowest` with a 2px "Electric Cyan" (`primary-container`) bottom-only indicator.
- **Error:** Background shifts to `error-container` (#fb5151) at 10% opacity; text remains high-contrast `error`.

### Cards & Lists

- **Constraint:** No divider lines. Use `spacing-6` (2rem) of vertical white space to separate items.
- **Interaction:** On hover, a card should transition from `surface` to `surface-container-lowest` with a subtle `24px` backdrop blur, creating a "lift" effect.

### Signature Component: The "Glass Header"

A persistent navigation bar using `surface-container-lowest` at 60% opacity with a heavy `32px` backdrop blur. This ensures the content "bleeds" through the header as the user scrolls, maintaining the atmospheric depth.

---

## 6. Do’s and Don’ts

### Do

- **Do** use asymmetrical layouts. Align a headline to the far left and the body text to a center-right column to create negative space.
- **Do** use `rounded-lg` (2rem) and `rounded-xl` (3rem) for large containers to soften the technical feel of the typography.
- **Do** overlap elements. Let a glass card sit 25% over a pearlescent background image.

### Don't

- **Don't** use pure black (#000000). Use `on-surface` (#2c2f31) for all "black" text to maintain the soft, light-mode airiness.
- **Don't** use standard Material Design "elevations." If it looks like a card with a 1px shadow, it is a failure of the system.
- **Don't** use tight spacing. When in doubt, increase the padding by one step on the Spacing Scale (e.g., move from `spacing-4` to `spacing-5`).

---

## 7. Spacing & Rhythm

The system relies on generous "Breathing Room."

- **Desktop Gutters:** Minimum `spacing-12` (4rem).
- **Component Padding:** Internal padding should never be less than `spacing-4` (1.4rem) to ensure the "Glass" has enough surface area to show its translucency.
