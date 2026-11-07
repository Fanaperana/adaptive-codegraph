; Python symbol extraction queries

; Functions / methods (methods are detected from the enclosing class)
(function_definition name: (identifier) @symbol.name return_type: (_)? @symbol.type) @symbol.def

; Classes
(class_definition name: (identifier) @symbol.name) @symbol.def

; Decorated definitions (span includes the decorators)
(decorated_definition definition: (function_definition name: (identifier) @symbol.name return_type: (_)? @symbol.type)) @symbol.def
(decorated_definition definition: (class_definition name: (identifier) @symbol.name)) @symbol.def

; Module-level assignments (constants, variables)
(module (expression_statement (assignment left: (identifier) @symbol.name type: (_)? @symbol.type) @symbol.def))

; Class-level attributes
(class_definition body: (block (expression_statement (assignment left: (identifier) @symbol.name type: (_)? @symbol.type) @symbol.def)))
