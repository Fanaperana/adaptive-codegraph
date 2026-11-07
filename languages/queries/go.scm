; Go symbol extraction queries

; Functions
(function_declaration name: (identifier) @symbol.name result: (_)? @symbol.type) @symbol.def

; Methods: the receiver type is the owner
(method_declaration
  receiver: (parameter_list (parameter_declaration type: [
    (type_identifier) @symbol.container
    (pointer_type (type_identifier) @symbol.container)
    (generic_type type: (type_identifier) @symbol.container)
    (pointer_type (generic_type type: (type_identifier) @symbol.container))
  ]))
  name: (field_identifier) @symbol.name
  result: (_)? @symbol.type) @symbol.def

; Type declarations (struct / interface / other)
(type_spec name: (type_identifier) @symbol.name) @symbol.def
(type_alias name: (type_identifier) @symbol.name) @symbol.def

; Struct fields and interface methods
(field_declaration name: (field_identifier) @symbol.name type: (_) @symbol.type) @symbol.def
(method_elem name: (field_identifier) @symbol.name result: (_)? @symbol.type) @symbol.def

; Package-level variables and constants
(source_file (var_declaration (var_spec name: (identifier) @symbol.name type: (_)? @symbol.type) @symbol.def))
(source_file (var_declaration (var_spec_list (var_spec name: (identifier) @symbol.name type: (_)? @symbol.type) @symbol.def)))
(source_file (const_declaration (const_spec name: (identifier) @symbol.name) @symbol.def))
