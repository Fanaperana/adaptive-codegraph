; C symbol extraction queries

; Function definitions (direct and pointer return types)
(function_definition type: (_) @symbol.type declarator: (function_declarator declarator: (identifier) @symbol.name)) @symbol.def
(function_definition type: (_) @symbol.type declarator: (pointer_declarator declarator: (function_declarator declarator: (identifier) @symbol.name))) @symbol.def

; Struct / union / enum definitions (only with a body: not every `struct X *p`)
(struct_specifier name: (type_identifier) @symbol.name body: (_)) @symbol.def
(union_specifier name: (type_identifier) @symbol.name body: (_)) @symbol.def
(enum_specifier name: (type_identifier) @symbol.name body: (_)) @symbol.def

; Struct fields
(field_declaration type: (_) @symbol.type declarator: (field_identifier) @symbol.name) @symbol.def
(field_declaration type: (_) @symbol.type declarator: (pointer_declarator declarator: (field_identifier) @symbol.name)) @symbol.def

; Enum values
(enumerator name: (identifier) @symbol.name) @symbol.def

; Typedefs
(type_definition declarator: (type_identifier) @symbol.name) @symbol.def

; Macros
(preproc_function_def name: (identifier) @symbol.name) @symbol.def
(preproc_def name: (identifier) @symbol.name) @symbol.def

; Global variables
(translation_unit (declaration type: (_) @symbol.type declarator: (init_declarator declarator: (identifier) @symbol.name)) @symbol.def)
(translation_unit (declaration type: (_) @symbol.type declarator: (identifier) @symbol.name) @symbol.def)
