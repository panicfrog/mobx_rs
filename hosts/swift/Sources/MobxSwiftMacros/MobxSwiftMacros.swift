import SwiftSyntax
import SwiftSyntaxBuilder
import SwiftCompilerPlugin

@main
struct MobxSwiftPlugin: CompilerPlugin {
    let providingMacros: [Macro.Type] = [
        ObservableMacro.self
    ]
}

public struct ObservableMacro: MemberMacro {
    public static func expansion( of node: AttributeSyntax,
                                  providingMembersOf declaration: some DeclGroupSyntax,
                                  in context: some MacroExpansionContext) throws -> [DeclSyntax] {
        return []
    }
}
