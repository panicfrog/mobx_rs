import SwiftDiagnostics
import SwiftSyntax
import SwiftSyntaxBuilder
import SwiftSyntaxMacros

enum MacroUtilities {
    static func error(_ message: String, node: Syntax) -> DiagnosticsError {
        DiagnosticsError(diagnostics: [
            Diagnostic(node: node, message: SimpleDiagnosticMessage(message: message))
        ])
    }
}

struct SimpleDiagnosticMessage: DiagnosticMessage {
    let message: String
    let diagnosticID = MessageID(domain: "MobxSwiftMacros", id: "error")
    let severity: DiagnosticSeverity = .error
}

public struct ActionMacro: ExpressionMacro {
    public static func expansion(
        of node: some FreestandingMacroExpansionSyntax,
        in context: some MacroExpansionContext
    ) throws -> ExprSyntax {
        guard let expression = Syntax(node).as(MacroExpansionExprSyntax.self) else {
            throw MacroUtilities.error("#mobxAction 只能用于表达式位置", node: Syntax(node))
        }

        let arguments = expression.arguments
        if arguments.isEmpty {
            throw MacroUtilities.error("#mobxAction 缺少参数", node: Syntax(node))
        }

        var runtimeExpr: ExprSyntax?
        var nameExpr: ExprSyntax?

        for element in arguments {
            guard let label = element.label?.text else {
                throw MacroUtilities.error("#mobxAction 参数需要标签", node: Syntax(element))
            }

            switch label {
            case "runtime":
                runtimeExpr = element.expression
            case "name":
                nameExpr = element.expression
            default:
                throw MacroUtilities.error("未知的 #mobxAction 参数 `\(label)`", node: Syntax(element))
            }
        }

        guard let runtimeExpr else {
            throw MacroUtilities.error("#mobxAction 需要传入 runtime", node: Syntax(node))
        }

        guard let body = expression.trailingClosure else {
            throw MacroUtilities.error("#mobxAction 需要尾随闭包", node: Syntax(node))
        }

        let nameValue = nameExpr ?? ExprSyntax("\"swift::action\"")
        let bodyClosure = body.trimmed

        let lowered: ExprSyntax =
            """
            \(runtimeExpr).runInAction(name: \(nameValue)) \(bodyClosure)
            """

        return lowered
    }
}
