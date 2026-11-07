; TypeScript symbol extraction queries (also used for TSX)

; Functions (any nesting level)
(function_declaration name: (identifier) @symbol.name return_type: (_)? @symbol.type) @symbol.def
(generator_function_declaration name: (identifier) @symbol.name return_type: (_)? @symbol.type) @symbol.def

; Classes, interfaces, types, enums
(class_declaration name: (type_identifier) @symbol.name) @symbol.def
(abstract_class_declaration name: (type_identifier) @symbol.name) @symbol.def
(interface_declaration name: (type_identifier) @symbol.name) @symbol.def
(type_alias_declaration name: (type_identifier) @symbol.name) @symbol.def
(enum_declaration name: (identifier) @symbol.name) @symbol.def
(enum_body (property_identifier) @symbol.name @symbol.def (#set! kind "enum_variant"))
(enum_assignment name: (property_identifier) @symbol.name) @symbol.def

; Class members
(method_definition name: (property_identifier) @symbol.name return_type: (_)? @symbol.type) @symbol.def
(abstract_method_signature name: (property_identifier) @symbol.name return_type: (_)? @symbol.type) @symbol.def
(public_field_definition name: (property_identifier) @symbol.name type: (_)? @symbol.type) @symbol.def

; Interface members
(method_signature name: (property_identifier) @symbol.name return_type: (_)? @symbol.type) @symbol.def
(property_signature name: (property_identifier) @symbol.name type: (_)? @symbol.type) @symbol.def

; Top-level variables (arrow functions become kind "function")
(program (lexical_declaration (variable_declarator name: (identifier) @symbol.name type: (_)? @symbol.type) @symbol.def))
(program (variable_declaration (variable_declarator name: (identifier) @symbol.name type: (_)? @symbol.type) @symbol.def))
(export_statement declaration: (lexical_declaration (variable_declarator name: (identifier) @symbol.name type: (_)? @symbol.type) @symbol.def))

; Exported declarations (span includes `export`)
(export_statement declaration: (function_declaration name: (identifier) @symbol.name return_type: (_)? @symbol.type)) @symbol.def
(export_statement declaration: (class_declaration name: (type_identifier) @symbol.name)) @symbol.def
(export_statement declaration: (abstract_class_declaration name: (type_identifier) @symbol.name)) @symbol.def
(export_statement declaration: (interface_declaration name: (type_identifier) @symbol.name)) @symbol.def
(export_statement declaration: (type_alias_declaration name: (type_identifier) @symbol.name)) @symbol.def
(export_statement declaration: (enum_declaration name: (identifier) @symbol.name)) @symbol.def

; Namespaces / modules
(module name: (identifier) @symbol.name) @symbol.def
(internal_module name: (identifier) @symbol.name) @symbol.def
