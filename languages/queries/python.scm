; Python symbol extraction queries

; Functions
(function_definition name: (identifier) @symbol.name) @symbol.def

; Classes
(class_definition name: (identifier) @symbol.name) @symbol.def

; Decorated definitions
(decorated_definition definition: (function_definition name: (identifier) @symbol.name)) @symbol.def
(decorated_definition definition: (class_definition name: (identifier) @symbol.name)) @symbol.def

; Module-level assignments (constants, variables)
(assignment left: (identifier) @symbol.name) @symbol.def
