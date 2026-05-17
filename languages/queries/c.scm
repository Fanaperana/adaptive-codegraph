; C symbol extraction queries

; Function definitions (direct return type)
(function_definition declarator: (function_declarator declarator: (identifier) @symbol.name)) @symbol.def

; Function definitions (pointer return type)
(function_definition declarator: (pointer_declarator declarator: (function_declarator declarator: (identifier) @symbol.name))) @symbol.def

; Struct definitions
(struct_specifier name: (type_identifier) @symbol.name) @symbol.def

; Struct fields
(field_declaration declarator: (field_identifier) @symbol.name) @symbol.def

; Enum definitions
(enum_specifier name: (type_identifier) @symbol.name) @symbol.def

; Enum values (enumerators)
(enumerator name: (identifier) @symbol.name) @symbol.def

; Union definitions
(union_specifier name: (type_identifier) @symbol.name) @symbol.def

; Type definitions
(type_definition declarator: (type_identifier) @symbol.name) @symbol.def

; Macro definitions
(preproc_function_def name: (identifier) @symbol.name) @symbol.def
(preproc_def name: (identifier) @symbol.name) @symbol.def

; Global variable declarations
(declaration declarator: (init_declarator declarator: (identifier) @symbol.name)) @symbol.def
