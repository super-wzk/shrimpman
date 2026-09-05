# Domain Language

This document defines Shrimpman's ubiquitous language. Domain and application
code should use these terms even when a reference implementation uses different
names. Infrastructure terms are called out separately.

## Server-selection hierarchy

```text
Sign Service
└── Entrance Service
    └── World
        └── Land
            └── Land Server
```

The hierarchy describes responsibility, not process ownership. A Land is the
client-visible destination; a Land Server is the process that serves it.

## Domain terms

| Term | Meaning |
| --- | --- |
| **World** | A client-visible group of Lands returned by the Entrance Service. |
| **World type** | The purpose of a World: Free, Dundorma Town, Beginner, Public Tavern, Returning Hunter, or Mezeporta Festa. |
| **World season** | The current Breeding, Warm, or Cold season of a World, inherited from Monster Hunter 2 (dos). |
| **World content** | The quest range or minigame content offered to characters entering a World. |
| **Land** | A client-visible destination within a World. It has connection and occupancy information and is backed by a Land Server. |
| **Character presence** | The known active World and Land for a requested character. An absent location means no active location is known. |

## Service terms

| Term | Responsibility |
| --- | --- |
| **Sign Service** | Authenticates accounts, issues sign sessions, and supplies an Entrance Service endpoint. |
| **Entrance Service** | Returns the available Worlds and Lands and, when requested, character presence. It does not host gameplay sessions. |
| **Land Server** | Accepts gameplay connections for a Land. |

Avoid the unqualified term `Server` when the specific service is known.
