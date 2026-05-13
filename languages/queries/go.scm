; Go symbol extraction queries

; Function declarations
(function_declaration name: (identifier) @symbol.name) @symbol.def

; Method declarations
(method_declaration name: (field_identifier) @symbol.name) @symbol.def

; Type declarations (struct, interface, alias)
(type_declaration (type_spec name: (type_identifier) @symbol.name)) @symbol.def
