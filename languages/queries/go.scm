; Go symbol extraction queries

; Function declarations
(function_declaration name: (identifier) @symbol.name) @symbol.def

; Method declarations
(method_declaration name: (field_identifier) @symbol.name) @symbol.def

; Type declarations (struct, interface, alias)
(type_declaration (type_spec name: (type_identifier) @symbol.name)) @symbol.def

; Struct field declarations
(field_declaration name: (field_identifier) @symbol.name) @symbol.def

; Package-level variable declarations
(var_declaration (var_spec name: (identifier) @symbol.name)) @symbol.def

; Package-level constant declarations
(const_declaration (const_spec name: (identifier) @symbol.name)) @symbol.def
