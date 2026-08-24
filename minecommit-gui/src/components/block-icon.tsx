import { cn } from "@/lib/utils"

/**
 * A grass block, drawn here rather than taken from anywhere.
 *
 * The dashboard is a list of Minecraft worlds and looked like a list of
 * folders, which is exactly the confusion that makes people close the app
 * unsure whether it found their saves at all. One block in front of each world
 * says "these are your worlds" faster than any label.
 *
 * Every shape and colour below is original. Minecraft's own textures are
 * Mojang's, and shipping them -- even the sixteen-pixel ones -- would be
 * putting somebody else's art in a program we hand out. This is the generic
 * idea of a grass-topped cube, which nobody owns: an isometric block, green on
 * top, soil down the sides, with the fringe of grass over the edge that makes
 * it readable at sixteen pixels.
 */
export function GrassBlock({
  className,
  title,
}: {
  className?: string
  title?: string
}) {
  return (
    <svg
      viewBox="0 0 16 16"
      className={cn("size-4 shrink-0", className)}
      role={title ? "img" : "presentation"}
      aria-label={title}
      aria-hidden={title ? undefined : true}
    >
      {/* Top: the grass surface, lit from above. */}
      <polygon points="8,1 15,4.5 8,8 1,4.5" fill="#7cb342" />
      <polygon points="8,1 15,4.5 8,8" fill="#71a53a" />
      {/* A few darker tufts, so the face is not a flat wash at small sizes. */}
      <polygon points="6,3.5 7,4 6,4.5 5,4" fill="#66992f" />
      <polygon points="10,5 11,5.5 10,6 9,5.5" fill="#66992f" />
      <polygon points="9,2.5 10,3 9,3.5 8,3" fill="#8bc34a" />

      {/* Left side: soil, with the grass hanging over the top edge. */}
      <polygon points="1,4.5 8,8 8,15 1,11.5" fill="#8d6e4a" />
      <polygon points="1,4.5 8,8 8,9.7 1,6.2" fill="#6b9c37" />
      <polygon points="2.6,7.6 4.1,8.4 4.1,9.6 2.6,8.8" fill="#7d6041" />
      <polygon points="5.3,10.2 6.4,10.8 6.4,12 5.3,11.4" fill="#7d6041" />

      {/* Right side: the same block turned away from the light. */}
      <polygon points="15,4.5 15,11.5 8,15 8,8" fill="#6f5638" />
      <polygon points="15,4.5 15,6.2 8,9.7 8,8" fill="#587f2c" />
      <polygon points="11.6,8.6 13.1,7.8 13.1,9 11.6,9.8" fill="#634c31" />
      <polygon points="9.4,11.6 10.5,11 10.5,12.2 9.4,12.8" fill="#634c31" />
    </svg>
  )
}
