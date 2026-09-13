use crate::ast::*;
use crate::error;
use crate::scanner::{Token, TokenType};

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, current: 0 }
    }

    pub fn parse(&mut self) -> Result<Expr, ()> {
        let result = self.expression()?;
        self.skip_newlines();

        if !self.is_at_end() && self.peek().token_type != TokenType::EOF {
            let token = self.peek().clone();
            error::syntax_at_token(token.line as u32, &token.lexeme, "expected expression");
            return Err(());
        }
        Ok(result)
    }

    fn expression(&mut self) -> Result<Expr, ()> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr, ()> {
        let expr = self.ternary()?;

        if let Some(op) = self.match_compound_assign_op() {
            let rhs = self.assignment()?;
            return self.finish_assignment(expr, op, rhs);
        }

        if self.match_types(&[TokenType::Equal]) {
            let value = self.assignment()?;
            return self.finish_plain_assignment(expr, value);
        }
        Ok(expr)
    }

    fn finish_plain_assignment(&self, target: Expr, value: Expr) -> Result<Expr, ()> {
        match target {
            Expr::Variable { name, .. } => Ok(Expr::Assign {
                name,
                value: Box::new(value),
            }),
            Expr::GetField { object, field } => Ok(Expr::SetField {
                object,
                field,
                value: Box::new(value),
            }),
            Expr::Index { object, index } => Ok(Expr::IndexSet {
                object,
                index,
                value: Box::new(value),
            }),
            _ => {
                let line = self.previous().line as u32;
                error::syntax_msg(line, "invalid assignment target");
                Err(())
            }
        }
    }

    fn match_compound_assign_op(&mut self) -> Option<BinOp> {
        Some(match self.peek().token_type {
            TokenType::PlusEqual => {
                self.advance();
                BinOp::Add
            }
            TokenType::MinusEqual => {
                self.advance();
                BinOp::Sub
            }
            TokenType::StarEqual => {
                self.advance();
                BinOp::Mul
            }
            TokenType::SlashEqual => {
                self.advance();
                BinOp::Div
            }
            TokenType::PercentEqual => {
                self.advance();
                BinOp::Rem
            }
            TokenType::StarStarEqual => {
                self.advance();
                BinOp::Pow
            }
            _ => return None,
        })
    }

    fn finish_assignment(&self, target: Expr, op: BinOp, rhs: Expr) -> Result<Expr, ()> {
        let value = Expr::Binary {
            op,
            left: Box::new(target.clone()),
            right: Box::new(rhs),
        };
        match target {
            Expr::Variable { name, .. } => Ok(Expr::Assign {
                name,
                value: Box::new(value),
            }),
            Expr::GetField { object, field } => Ok(Expr::SetField {
                object,
                field,
                value: Box::new(value),
            }),
            Expr::Index { object, index } => Ok(Expr::IndexSet {
                object,
                index,
                value: Box::new(value),
            }),
            _ => {
                let line = self.previous().line as u32;
                error::syntax_msg(line, "invalid assignment target");
                Err(())
            }
        }
    }

    fn ternary(&mut self) -> Result<Expr, ()> {
        let expr = self.or_expr()?;

        if self.match_types(&[TokenType::If]) {
            let condition = self.or_expr()?;

            if !self.match_types(&[TokenType::Else]) {
                let line = self.peek().line as u32;
                error::syntax_msg(line, "expected 'else' in ternary expression");
                return Err(());
            }

            let else_expr = self.ternary()?;
            return Ok(Expr::Ternary {
                condition: Box::new(condition),
                then_branch: Box::new(expr),
                else_branch: Box::new(else_expr),
            });
        }

        Ok(expr)
    }

    fn or_expr(&mut self) -> Result<Expr, ()> {
        let mut expr = self.and_expr()?;
        while self.match_types(&[TokenType::Or]) {
            let right = self.and_expr()?;
            expr = Expr::Binary {
                op: BinOp::Or,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn and_expr(&mut self) -> Result<Expr, ()> {
        let mut expr = self.equality()?;
        while self.match_types(&[TokenType::And]) {
            let right = self.equality()?;
            expr = Expr::Binary {
                op: BinOp::And,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn equality(&mut self) -> Result<Expr, ()> {
        let mut expr = self.comparison()?;
        while self.match_types(&[TokenType::BangEqual, TokenType::EqualEqual]) {
            let op = match self.previous().token_type {
                TokenType::BangEqual => BinOp::Ne,
                _ => BinOp::Eq,
            };
            let right = self.comparison()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn comparison(&mut self) -> Result<Expr, ()> {
        let mut expr = self.term()?;
        while self.match_types(&[
            TokenType::Greater,
            TokenType::GreaterEqual,
            TokenType::Less,
            TokenType::LessEqual,
        ]) {
            let op = match self.previous().token_type {
                TokenType::Greater => BinOp::Gt,
                TokenType::GreaterEqual => BinOp::Ge,
                TokenType::Less => BinOp::Lt,
                _ => BinOp::Le,
            };
            let right = self.term()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn term(&mut self) -> Result<Expr, ()> {
        let mut expr = self.factor()?;
        while self.match_types(&[TokenType::Minus, TokenType::Plus]) {
            let op = match self.previous().token_type {
                TokenType::Minus => BinOp::Sub,
                _ => BinOp::Add,
            };
            let right = self.factor()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn factor(&mut self) -> Result<Expr, ()> {
        let mut expr = self.power()?;
        while self.match_types(&[
            TokenType::Slash,
            TokenType::FloorDiv,
            TokenType::Star,
            TokenType::Percent,
        ]) {
            let op = match self.previous().token_type {
                TokenType::Slash => BinOp::Div,
                TokenType::FloorDiv => BinOp::FloorDiv,
                TokenType::Percent => BinOp::Rem,
                _ => BinOp::Mul,
            };
            let right = self.power()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn power(&mut self) -> Result<Expr, ()> {
        let mut expr = self.unary()?;
        while self.match_types(&[TokenType::StarStar]) {
            let right = self.unary()?;
            expr = Expr::Binary {
                op: BinOp::Pow,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn unary(&mut self) -> Result<Expr, ()> {
        if self.match_types(&[TokenType::Handle]) {
            let attempt = self.unary()?;
            self.consume(TokenType::Else, "Expect 'else' after handle expression.")?;
            let fallback = self.unary()?;
            return Ok(Expr::Handle {
                attempt: Box::new(attempt),
                fallback: Box::new(fallback),
            });
        }
        if self.match_types(&[TokenType::Bang, TokenType::Minus, TokenType::Not]) {
            let op = match self.previous().token_type {
                TokenType::Not | TokenType::Bang => UnaryOp::Not,
                _ => UnaryOp::Neg,
            };
            let right = self.unary()?;
            return Ok(Expr::Unary {
                op,
                right: Box::new(right),
            });
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, ()> {
        let mut expr = self.primary()?;
        loop {
            if self.match_types(&[TokenType::LeftBracket]) {
                let index = self.expression()?;
                self.consume(TokenType::RightBracket, "Expect ']' after index.")?;
                expr = Expr::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                };
            } else if self.match_types(&[TokenType::Dot]) {
                let member_name =
                    self.consume_member_name("Expect property or method name after '.'")?;
                if self.match_types(&[TokenType::LeftParen]) {
                    let mut args = Vec::new();
                    if !self.check(&TokenType::RightParen) {
                        loop {
                            args.push(self.expression()?);
                            if !self.match_types(&[TokenType::Comma]) {
                                break;
                            }
                        }
                    }
                    self.consume(TokenType::RightParen, "Expect ')' after arguments.")?;
                    expr = Expr::CallMethod {
                        object: Box::new(expr),
                        method: member_name,
                        args,
                    };
                } else if let Expr::Variable { name, .. } = expr {
                    expr = Expr::GetField {
                        object: name,
                        field: member_name,
                    };
                } else {
                    error::syntax_msg(
                        self.peek().line as u32,
                        "field access requires a named variable",
                    );
                    return Err(());
                }
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn skip_layout_tokens(&mut self) {
        while self.match_types(&[TokenType::Newline, TokenType::Indent, TokenType::Dedent]) {}
    }

    fn parse_collection_after_open(
        &mut self,
        close: TokenType,
        close_msg: &str,
    ) -> Result<Expr, ()> {
        self.skip_layout_tokens();
        if self.check(&close) {
            self.advance();
            return Ok(if close == TokenType::RightBrace {
                Expr::Dict(Vec::new())
            } else {
                Expr::List(Vec::new())
            });
        }

        let first_key = self.expression()?;

        if self.match_types(&[TokenType::Colon]) {
            let first_val = self.expression()?;
            let mut entries = vec![(first_key, first_val)];

            while self.match_types(&[TokenType::Comma]) {
                self.skip_layout_tokens();
                if self.check(&close) {
                    break;
                }
                let key = self.expression()?;
                self.consume(TokenType::Colon, "Expect ':' after dictionary key.")?;
                let value = self.expression()?;
                entries.push((key, value));
            }

            self.skip_layout_tokens();
            self.consume(close, close_msg)?;
            return Ok(Expr::Dict(entries));
        }

        let mut elements = vec![first_key];
        while self.match_types(&[TokenType::Comma]) {
            self.skip_layout_tokens();
            if self.check(&close) {
                break;
            }
            elements.push(self.expression()?);
        }

        self.skip_layout_tokens();
        self.consume(close, close_msg)?;
        Ok(Expr::List(elements))
    }

    fn primary(&mut self) -> Result<Expr, ()> {
        if self.match_types(&[TokenType::False]) {
            return Ok(Expr::Literal(Literal::Bool(false)));
        }
        if self.match_types(&[TokenType::True]) {
            return Ok(Expr::Literal(Literal::Bool(true)));
        }
        if self.match_types(&[TokenType::None]) {
            return Ok(Expr::Literal(Literal::None));
        }

        if self.match_types(&[TokenType::LeftBrace]) {
            return self.parse_collection_after_open(
                TokenType::RightBrace,
                "Expect '}' after collection.",
            );
        }

        if self.match_types(&[TokenType::LeftBracket]) {
            return self.parse_collection_after_open(
                TokenType::RightBracket,
                "Expect ']' after collection.",
            );
        }

        if self.match_types(&[TokenType::Input]) {
            let line = self.previous().line as u32;
            let mut args = Vec::new();

            self.consume(TokenType::LeftParen, "Expect '(' after input.")?;
            if !self.check(&TokenType::RightParen) {
                args.push(CallArg::Positional(self.expression()?));
            }
            self.consume(TokenType::RightParen, "Expect ')' after input argument.")?;

            return Ok(Expr::Call {
                callee: Box::new(Expr::Variable {
                    name: "input".to_string(),
                    line,
                }),
                args,
            });
        }

        // `range` is a keyword used by `for i in range(...)`; also allow `range(...)` as a list builtin.
        if self.check(&TokenType::Range) && self.check_next(&TokenType::LeftParen) {
            let line = self.peek().line as u32;
            self.advance(); // Range
            let mut args = Vec::new();
            self.consume(TokenType::LeftParen, "Expect '(' after range.")?;
            if !self.check(&TokenType::RightParen) {
                args.push(CallArg::Positional(self.expression()?));
                while self.match_types(&[TokenType::Comma]) {
                    if self.check(&TokenType::RightParen) {
                        break;
                    }
                    args.push(CallArg::Positional(self.expression()?));
                }
            }
            self.consume(TokenType::RightParen, "Expect ')' after range arguments.")?;
            return Ok(Expr::Call {
                callee: Box::new(Expr::Variable {
                    name: "range".to_string(),
                    line,
                }),
                args,
            });
        }

        // `bool` is a type keyword; allow `bool(x)` as the conversion builtin (like `input()`).
        if self.check(&TokenType::TypeBool) && self.check_next(&TokenType::LeftParen) {
            let line = self.peek().line as u32;
            self.advance(); // TypeBool
            let mut args = Vec::new();
            self.consume(TokenType::LeftParen, "Expect '(' after bool.")?;
            if !self.check(&TokenType::RightParen) {
                args.push(CallArg::Positional(self.expression()?));
                while self.match_types(&[TokenType::Comma]) {
                    if self.check(&TokenType::RightParen) {
                        break;
                    }
                    args.push(CallArg::Positional(self.expression()?));
                }
            }
            self.consume(TokenType::RightParen, "Expect ')' after bool argument.")?;
            return Ok(Expr::Call {
                callee: Box::new(Expr::Variable {
                    name: "bool".to_string(),
                    line,
                }),
                args,
            });
        }

        if self.match_types(&[TokenType::FString]) {
            let f_content = self.previous().literal.clone();
            let line = self.previous().line;
            return self.parse_f_string(&f_content, line);
        }

        if self.match_types(&[TokenType::Identifier]) {
            let name = self.previous().lexeme.clone();
            let line = self.previous().line as u32;

            if self.match_types(&[TokenType::LeftParen]) {
                let mut args = Vec::new();
                if !self.check(&TokenType::RightParen) {
                    loop {
                        args.push(self.parse_call_argument()?);
                        if !self.match_types(&[TokenType::Comma]) {
                            break;
                        }
                    }
                }
                self.consume(TokenType::RightParen, "Expect ')' after arguments.")?;

                return Ok(Expr::Call {
                    callee: Box::new(Expr::Variable { name, line }),
                    args,
                });
            }

            return Ok(Expr::Variable { name, line });
        }

        if self.match_types(&[TokenType::String]) {
            return Ok(Expr::Literal(Literal::String(
                self.previous().literal.clone(),
            )));
        }

        if self.match_types(&[TokenType::Number]) {
            return Ok(Expr::Literal(Literal::Number(
                self.previous().literal.clone(),
            )));
        }

        if self.match_types(&[TokenType::LeftParen]) {
            let expr = self.expression()?;
            self.consume(TokenType::RightParen, "Expect ')' after expression.")?;
            return Ok(Expr::Group(Box::new(expr)));
        }

        let token = self.peek().clone();
        if token.token_type == TokenType::EOF {
            error::syntax_msg(token.line as u32, "expected expression");
        } else {
            error::syntax_at_token(token.line as u32, &token.lexeme, "expected expression");
        }
        Err(())
    }

    fn skip_newlines(&mut self) {
        while self.check(&TokenType::Newline) {
            self.advance();
        }
    }

    fn is_type_token(token_type: &TokenType) -> bool {
        matches!(
            token_type,
            TokenType::Identifier
                | TokenType::TypeI8
                | TokenType::TypeI16
                | TokenType::TypeI32
                | TokenType::TypeI64
                | TokenType::TypeU8
                | TokenType::TypeU16
                | TokenType::TypeU32
                | TokenType::TypeU64
                | TokenType::TypeF32
                | TokenType::TypeF64
                | TokenType::TypeString
                | TokenType::TypeBool
                | TokenType::None
                | TokenType::Array
                | TokenType::Dict
        )
    }

    fn parse_type_ann(&mut self) -> Result<TypeAnn, ()> {
        if self.match_types(&[TokenType::Array]) {
            self.consume(TokenType::LeftBracket, "Expect '[' after 'Array'.")?;
            let inner_type = self.consume_type_name("Expect array element type.")?;
            self.consume(TokenType::RightBracket, "Expect ']' after array type.")?;
            Ok(TypeAnn::Array { inner: inner_type })
        } else if self.match_types(&[TokenType::Dict]) {
            self.consume(TokenType::LeftBracket, "Expect '[' after 'Dict'.")?;
            let key_type = self.consume_type_name("Expect dictionary key type.")?;
            self.consume(
                TokenType::Comma,
                "Expect ',' between dictionary key and value types.",
            )?;
            let val_type = self.consume_type_name("Expect dictionary value type.")?;
            self.consume(
                TokenType::RightBracket,
                "Expect ']' after dictionary types.",
            )?;
            Ok(TypeAnn::Dict {
                key: key_type,
                value: val_type,
            })
        } else {
            let type_name = self.consume_type_name("Expect type name.")?;
            Ok(TypeAnn::Named(type_name))
        }
    }

    fn type_ann_to_string(ann: &TypeAnn) -> String {
        match ann {
            TypeAnn::None => String::new(),
            TypeAnn::Named(name) => name.clone(),
            TypeAnn::Array { inner } => format!("Array[{}]", inner),
            TypeAnn::Dict { key, value } => format!("Dict[{}, {}]", key, value),
        }
    }

    /// `name: Array[i32] = [...]` without a leading `let`.
    fn looks_like_typed_binding(&self) -> bool {
        let mut i = self.current;
        if i >= self.tokens.len() || self.tokens[i].token_type != TokenType::Identifier {
            return false;
        }
        i += 1;
        if i >= self.tokens.len() || self.tokens[i].token_type != TokenType::Colon {
            return false;
        }
        i += 1;
        i = match self.type_annotation_end(i) {
            Some(end) => end,
            None => return false,
        };
        i < self.tokens.len() && self.tokens[i].token_type == TokenType::Equal
    }

    fn type_annotation_end(&self, mut i: usize) -> Option<usize> {
        if i >= self.tokens.len() {
            return None;
        }
        match &self.tokens[i].token_type {
            TokenType::Array => {
                i += 1;
                if i >= self.tokens.len() || self.tokens[i].token_type != TokenType::LeftBracket {
                    return None;
                }
                i += 1;
                while i < self.tokens.len()
                    && !matches!(
                        self.tokens[i].token_type,
                        TokenType::RightBracket | TokenType::Comma
                    )
                {
                    i += 1;
                }
                if i >= self.tokens.len() || self.tokens[i].token_type != TokenType::RightBracket {
                    return None;
                }
                Some(i + 1)
            }
            TokenType::Dict => {
                i += 1;
                if i >= self.tokens.len() || self.tokens[i].token_type != TokenType::LeftBracket {
                    return None;
                }
                i += 1;
                while i < self.tokens.len() && self.tokens[i].token_type != TokenType::RightBracket
                {
                    i += 1;
                }
                if i >= self.tokens.len() || self.tokens[i].token_type != TokenType::RightBracket {
                    return None;
                }
                Some(i + 1)
            }
            ty if Self::is_type_token(ty) => Some(i + 1),
            _ => None,
        }
    }

    fn typed_binding_declaration(&mut self) -> Result<Stmt, ()> {
        let line = self.peek().line as u32;
        let name_token = self
            .consume(TokenType::Identifier, "Expect variable name.")?
            .clone();
        self.consume(TokenType::Colon, "Expect ':' after variable name.")?;
        let type_ann = self.parse_type_ann()?;
        self.consume(TokenType::Equal, "Expect '=' after type annotation.")?;
        let initializer = self.expression()?;
        Ok(Stmt::Let {
            line,
            is_mutable: false,
            name: name_token.lexeme,
            type_ann,
            initializer,
        })
    }

    fn consume_type_name(&mut self, message: &str) -> Result<String, ()> {
        let token = self.peek().clone();
        if Self::is_type_token(&token.token_type) {
            self.advance();
            return Ok(token.lexeme);
        }
        if token.token_type == TokenType::EOF {
            error::syntax_msg(token.line as u32, message);
        } else {
            error::syntax_at_token(token.line as u32, &token.lexeme, message);
        }
        Err(())
    }

    fn consume_member_name(&mut self, message: &str) -> Result<String, ()> {
        let token = self.peek().clone();
        match token.token_type {
            TokenType::Identifier
            | TokenType::ReadChunk
            | TokenType::Input
            | TokenType::Print
            | TokenType::OpenMmap => {
                self.advance();
                Ok(token.lexeme)
            }
            _ => {
                if token.token_type == TokenType::EOF {
                    error::syntax_msg(token.line as u32, message);
                } else {
                    error::syntax_at_token(token.line as u32, &token.lexeme, message);
                }
                Err(())
            }
        }
    }

    fn match_types(&mut self, types: &[TokenType]) -> bool {
        for t in types {
            if self.check(t) {
                self.advance();
                return true;
            }
        }
        false
    }

    fn check(&self, token_type: &TokenType) -> bool {
        if self.is_at_end() {
            return false;
        }
        &self.peek().token_type == token_type
    }

    fn check_next(&self, token_type: &TokenType) -> bool {
        let next = self.current + 1;
        if next >= self.tokens.len() {
            return false;
        }
        &self.tokens[next].token_type == token_type
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.previous()
    }

    fn is_at_end(&self) -> bool {
        self.peek().token_type == TokenType::EOF
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.current - 1]
    }

    fn consume(&mut self, token_type: TokenType, message: &str) -> Result<&Token, ()> {
        if self.check(&token_type) {
            return Ok(self.advance());
        }
        let token = self.peek();

        if token.token_type == TokenType::EOF {
            error::syntax_msg(token.line as u32, message);
        } else {
            error::syntax_at_token(token.line as u32, &token.lexeme, message);
        }
        Err(())
    }

    pub fn parse_statements(&mut self) -> Result<Vec<Stmt>, ()> {
        let mut statements = Vec::new();
        while !self.is_at_end() && self.peek().token_type != TokenType::EOF {
            self.skip_newlines();
            if self.is_at_end() || self.peek().token_type == TokenType::EOF {
                break;
            }
            statements.push(self.declaration()?);
        }
        Ok(statements)
    }

    fn declaration(&mut self) -> Result<Stmt, ()> {
        let mut is_parallel = false;
        let mut is_vectorized = false;

        while self.match_types(&[TokenType::At]) {
            let dec_token = self
                .consume(TokenType::Identifier, "Expect decorator name after '@'.")?
                .clone();
            match dec_token.lexeme.as_str() {
                "parallel" => is_parallel = true,
                "vectorize" => is_vectorized = true,
                _ => {
                    error::syntax_at_token(
                        dec_token.line as u32,
                        &dec_token.lexeme,
                        &format!("unknown decorator '@{}'", dec_token.lexeme),
                    );
                    return Err(());
                }
            }
            if self.check(&TokenType::Newline) {
                self.advance();
            }
        }

        if self.match_types(&[TokenType::Import]) {
            return self.import_statement();
        }

        if self.match_types(&[TokenType::From]) {
            return self.import_from_statement();
        }

        if self.match_types(&[TokenType::Struct]) {
            return self.struct_declaration();
        }

        if self.match_types(&[TokenType::Trait]) {
            return self.trait_declaration();
        }

        if self.match_types(&[TokenType::Fn]) {
            return Ok(Stmt::Function(self.function_declaration(true)?));
        }

        if self.match_types(&[TokenType::Def]) {
            return Ok(Stmt::Function(self.function_declaration(false)?));
        }

        if self.match_types(&[TokenType::Let]) {
            return self.let_declaration();
        }

        if self.looks_like_typed_binding() {
            return self.typed_binding_declaration();
        }

        if self.match_types(&[TokenType::For]) {
            return self.for_statement(is_parallel, is_vectorized);
        }

        self.statement()
    }

    fn import_statement(&mut self) -> Result<Stmt, ()> {
        let line = self.previous().line as u32;
        let module = self
            .consume(TokenType::Identifier, "Expect module name after 'import'.")?
            .lexeme
            .clone();

        let alias = if self.match_types(&[TokenType::As]) {
            Some(
                self.consume(TokenType::Identifier, "Expect alias after 'as'.")?
                    .lexeme
                    .clone(),
            )
        } else {
            None
        };

        if self.check(&TokenType::Newline) {
            self.advance();
        }

        Ok(Stmt::Import {
            line,
            module,
            alias,
        })
    }

    fn import_from_statement(&mut self) -> Result<Stmt, ()> {
        let line = self.previous().line as u32;
        let module = self
            .consume(TokenType::Identifier, "Expect module name after 'from'.")?
            .lexeme
            .clone();

        self.consume(TokenType::Import, "Expect 'import' after module name.")?;

        let mut names = Vec::new();
        loop {
            let name = self
                .consume(TokenType::Identifier, "Expect name to import.")?
                .lexeme
                .clone();
            let alias = if self.match_types(&[TokenType::As]) {
                Some(
                    self.consume(TokenType::Identifier, "Expect alias after 'as'.")?
                        .lexeme
                        .clone(),
                )
            } else {
                None
            };
            names.push(ImportName { name, alias });

            if !self.match_types(&[TokenType::Comma]) {
                break;
            }
        }

        if names.is_empty() {
            error::syntax_msg(line, "expected at least one name after 'import'");
            return Err(());
        }

        if self.check(&TokenType::Newline) {
            self.advance();
        }

        Ok(Stmt::ImportFrom {
            line,
            module,
            names,
        })
    }

    fn struct_declaration(&mut self) -> Result<Stmt, ()> {
        let name_token = self
            .consume(TokenType::Identifier, "Expect struct name.")?
            .clone();
        let struct_name = name_token.lexeme;

        let mut implemented_trait = None;
        if self.match_types(&[TokenType::LeftParen]) {
            let trait_name = self
                .consume(
                    TokenType::Identifier,
                    "Expect trait name inside parentheses.",
                )?
                .lexeme
                .clone();
            self.consume(TokenType::RightParen, "Expect ')' after trait name.")?;
            implemented_trait = Some(trait_name);
        }

        self.consume(TokenType::Colon, "Expect ':' before struct body.")?;
        self.skip_newlines();
        self.consume(TokenType::Indent, "Expect indented block for struct body.")?;

        let mut fields = Vec::new();
        let mut methods = Vec::new();

        while !self.check(&TokenType::Dedent) && !self.is_at_end() {
            self.skip_newlines();
            if self.check(&TokenType::Dedent) || self.is_at_end() {
                break;
            }
            if self.match_types(&[TokenType::Let]) {
                let is_pub = self.match_types(&[TokenType::Pub]);
                let is_mutable = self.match_types(&[TokenType::Mut]);
                let field_name = self
                    .consume(TokenType::Identifier, "Expect field name.")?
                    .lexeme
                    .clone();
                self.consume(TokenType::Colon, "Expect ':' after field name.")?;
                let field_type = self.consume_type_name("Expect field type.")?;

                fields.push(StructField {
                    name: field_name,
                    type_name: field_type,
                    is_pub,
                    is_mut: is_mutable,
                });

                if self.check(&TokenType::Newline) {
                    self.advance();
                }
            } else if self.check(&TokenType::Pub)
                || self.check(&TokenType::Fn)
                || self.check(&TokenType::Def)
            {
                // `pub` precedes the `fn` / `def` keyword it applies to.
                let is_pub = self.match_types(&[TokenType::Pub]);
                if self.match_types(&[TokenType::Fn]) {
                    let function = self.function_declaration(true)?;
                    methods.push(MethodDecl { is_pub, function });
                } else if self.match_types(&[TokenType::Def]) {
                    let function = self.function_declaration(false)?;
                    methods.push(MethodDecl { is_pub, function });
                } else {
                    let token = self.peek().clone();
                    error::syntax_at_token(
                        token.line as u32,
                        &token.lexeme,
                        "expected 'fn' or 'def' after 'pub'",
                    );
                    return Err(());
                }
            } else if self.check(&TokenType::Newline) {
                self.advance();
            } else {
                self.advance();
            }
        }

        self.consume(TokenType::Dedent, "Expect dedent after struct body.")?;

        Ok(Stmt::Struct {
            name: struct_name,
            implemented_trait,
            fields,
            methods,
        })
    }

    fn trait_declaration(&mut self) -> Result<Stmt, ()> {
        let name_token = self
            .consume(TokenType::Identifier, "Expect trait name.")?
            .clone();
        let trait_name = name_token.lexeme;

        self.consume(TokenType::Colon, "Expect ':' before trait body.")?;
        self.skip_newlines();
        self.consume(TokenType::Indent, "Expect indented block for trait body.")?;

        let mut methods = Vec::new();
        while !self.check(&TokenType::Dedent) && !self.is_at_end() {
            self.skip_newlines();
            if self.check(&TokenType::Dedent) || self.is_at_end() {
                break;
            }
            if self.match_types(&[TokenType::Fn]) {
                methods.push(self.function_declaration(true)?);
            } else if self.match_types(&[TokenType::Def]) {
                methods.push(self.function_declaration(false)?);
            } else {
                self.advance();
            }
        }

        self.consume(TokenType::Dedent, "Expect dedent after trait body.")?;
        Ok(Stmt::Trait {
            name: trait_name,
            methods,
        })
    }

    fn function_declaration(&mut self, is_strict: bool) -> Result<FunctionDecl, ()> {
        let name_token = self
            .consume(TokenType::Identifier, "Expect function name.")?
            .clone();
        let fn_name = name_token.lexeme;

        self.consume(TokenType::LeftParen, "Expect '(' after function name.")?;

        let mut params = Vec::new();
        if !self.check(&TokenType::RightParen) {
            loop {
                let is_ref = self.match_types(&[TokenType::Ref]);
                let param_token = self.consume(TokenType::Identifier, "Expect parameter name.")?;
                let param_name = param_token.lexeme.clone();

                let type_ann = if self.match_types(&[TokenType::Colon]) {
                    Some(Self::type_ann_to_string(&self.parse_type_ann()?))
                } else {
                    None
                };

                params.push(Param {
                    name: param_name,
                    is_ref,
                    type_ann,
                });

                if !self.match_types(&[TokenType::Comma]) {
                    break;
                }
            }
        }
        self.consume(TokenType::RightParen, "Expect ')' after parameters.")?;

        // Accept `raises` before or after the return type:
        //   fn f() raises:
        //   fn f() -> i64 raises:
        //   fn f() raises -> i64:
        let mut raises = self.match_types(&[TokenType::Raises]);
        let return_type = if self.match_types(&[TokenType::Arrow]) {
            Some(Self::type_ann_to_string(&self.parse_type_ann()?))
        } else {
            None
        };
        if !raises {
            raises = self.match_types(&[TokenType::Raises]);
        }

        let body = if self.match_types(&[TokenType::Colon]) {
            self.skip_newlines();
            if self.check(&TokenType::Dedent)
                || self.check(&TokenType::EOF)
                || self.check(&TokenType::Fn)
                || self.check(&TokenType::Def)
            {
                Stmt::Block(Vec::new())
            } else {
                self.statement()?
            }
        } else {
            Stmt::Block(Vec::new())
        };

        Ok(FunctionDecl {
            name: fn_name,
            is_strict,
            params,
            return_type,
            raises,
            body: Box::new(body),
        })
    }

    fn parse_call_argument(&mut self) -> Result<CallArg, ()> {
        if self.check(&TokenType::Identifier) {
            let next_idx = self.current + 1;
            if next_idx < self.tokens.len() && self.tokens[next_idx].token_type == TokenType::Colon
            {
                let name = self.advance().lexeme.clone();
                self.advance(); // colon
                let value = self.expression()?;
                return Ok(CallArg::Named { name, value });
            }
        }
        Ok(CallArg::Positional(self.expression()?))
    }

    fn let_declaration(&mut self) -> Result<Stmt, ()> {
        let line = self.peek().line as u32;
        let is_mutable = self.match_types(&[TokenType::Mut]);

        let name_token = self
            .consume(TokenType::Identifier, "Expect variable name.")?
            .clone();
        let let_name = name_token.lexeme;

        let type_ann = if self.match_types(&[TokenType::Colon]) {
            self.parse_type_ann()?
        } else {
            TypeAnn::None
        };

        let initializer = if self.match_types(&[TokenType::Equal]) {
            self.expression()?
        } else {
            Expr::Literal(Literal::None)
        };

        Ok(Stmt::Let {
            line,
            is_mutable,
            name: let_name,
            type_ann,
            initializer,
        })
    }

    fn statement(&mut self) -> Result<Stmt, ()> {
        if self.match_types(&[TokenType::With]) {
            if self.check(&TokenType::OpenMmap) {
                return self.with_mmap_statement();
            }
            return self.with_statement();
        }

        if self.match_types(&[TokenType::Return]) {
            return self.return_statement();
        }

        if self.match_types(&[TokenType::Break]) {
            let line = self.previous().line as u32;
            if self.check(&TokenType::Newline) {
                self.advance();
            }
            return Ok(Stmt::Break { line });
        }

        if self.match_types(&[TokenType::Continue]) {
            let line = self.previous().line as u32;
            if self.check(&TokenType::Newline) {
                self.advance();
            }
            return Ok(Stmt::Continue { line });
        }

        if self.match_types(&[TokenType::Raise]) {
            let line = self.previous().line as u32;
            let value = self.expression()?;
            if self.check(&TokenType::Newline) {
                self.advance();
            }
            return Ok(Stmt::Raise { line, value });
        }

        if self.match_types(&[TokenType::Print]) {
            return self.print_statement();
        }

        if self.check(&TokenType::Indent) {
            return self.block();
        }

        if self.match_types(&[TokenType::If]) {
            return self.if_statement();
        }

        if self.match_types(&[TokenType::While]) {
            return self.while_statement();
        }

        if self.match_types(&[TokenType::For]) {
            return self.for_statement(false, false);
        }

        self.expression_statement()
    }

    fn with_mmap_statement(&mut self) -> Result<Stmt, ()> {
        let line = self.previous().line as u32;
        self.consume(TokenType::OpenMmap, "Expect 'open_mmap after 'with'.")?;
        self.consume(TokenType::LeftParen, "Expect '(' after open_mmap.")?;
        let path_expr = self.expression()?;
        self.consume(TokenType::RightParen, "Expect ')' after path.")?;

        self.consume(TokenType::As, "Expect 'as' in mmap block.")?;
        let var_token = self
            .consume(TokenType::Identifier, "Expect variable name after 'as'.")?
            .clone();
        let var_name = var_token.lexeme;

        if self.check(&TokenType::Colon) {
            self.advance();
        }
        self.skip_newlines();

        let body = self.statement()?;
        Ok(Stmt::WithMmap {
            line,
            path: path_expr,
            var: var_name,
            body: Box::new(body),
        })
    }

    /// `with <expr> as name:` — the resource form used by `open(...)`.
    fn with_statement(&mut self) -> Result<Stmt, ()> {
        let line = self.previous().line as u32;
        let value = self.expression()?;

        self.consume(TokenType::As, "Expect 'as' in with block.")?;
        let var_token = self
            .consume(TokenType::Identifier, "Expect variable name after 'as'.")?
            .clone();

        if self.check(&TokenType::Colon) {
            self.advance();
        }
        self.skip_newlines();

        let body = self.statement()?;
        Ok(Stmt::With {
            line,
            value,
            var: var_token.lexeme,
            body: Box::new(body),
        })
    }

    fn parse_f_string(&mut self, content: &str, line: usize) -> Result<Expr, ()> {
        let mut parts = Vec::new();
        let mut chars = content.chars().peekable();
        let mut current_text = String::new();

        while let Some(ch) = chars.next() {
            if ch == '\\' {
                if let Some(next_ch) = chars.next() {
                    current_text.push('\\');
                    current_text.push(next_ch);
                } else {
                    current_text.push('\\');
                }
            } else if ch == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    current_text.push('{');
                    continue;
                }

                if !current_text.is_empty() {
                    parts.push(FStringPart::Literal(current_text.clone()));
                    current_text.clear();
                }

                let mut expr_str = String::new();
                let mut brace_depth = 1;
                let mut in_string = false;
                let mut string_char = ' ';

                while let Some(c) = chars.next() {
                    if in_string {
                        expr_str.push(c);
                        if c == string_char {
                            in_string = false;
                        }
                    } else if c == '"' || c == '\'' {
                        in_string = true;
                        string_char = c;
                        expr_str.push(c);
                    } else if c == '{' {
                        brace_depth += 1;
                        expr_str.push(c);
                    } else if c == '}' {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            break;
                        } else {
                            expr_str.push(c);
                        }
                    } else {
                        expr_str.push(c);
                    }
                }

                if brace_depth > 0 {
                    error::syntax_msg(line as u32, "unterminated expression in f-string");
                    return Err(());
                }

                let (sub_tokens, err) = crate::scanner::scan_tokens(&expr_str);
                if err {
                    error::syntax_msg(
                        line as u32,
                        &format!(
                            "failed to parse expression inside f-string: '{}'",
                            expr_str
                        ),
                    );
                    return Err(());
                }
                let mut sub_parser = Parser::new(sub_tokens);
                let sub_ast = sub_parser.expression()?;
                parts.push(FStringPart::Expr(sub_ast));
            } else if ch == '}' {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    current_text.push('}');
                    continue;
                }
                current_text.push('}');
            } else {
                current_text.push(ch);
            }
        }

        if !current_text.is_empty() {
            parts.push(FStringPart::Literal(current_text));
        }

        Ok(Expr::FString {
            line: line as u32,
            parts,
        })
    }

    fn if_statement(&mut self) -> Result<Stmt, ()> {
        let condition = self.expression()?;

        if self.check(&TokenType::Colon) {
            self.advance();
        }
        self.skip_newlines();

        let then_branch = self.statement()?;

        if self.match_types(&[TokenType::Elif]) {
            let else_branch = self.if_statement()?;
            Ok(Stmt::If {
                condition,
                then_branch: Box::new(then_branch),
                else_branch: Some(Box::new(else_branch)),
            })
        } else if self.match_types(&[TokenType::Else]) {
            if self.check(&TokenType::Colon) {
                self.advance();
            }
            self.skip_newlines();
            let else_branch = self.statement()?;
            Ok(Stmt::If {
                condition,
                then_branch: Box::new(then_branch),
                else_branch: Some(Box::new(else_branch)),
            })
        } else {
            Ok(Stmt::If {
                condition,
                then_branch: Box::new(then_branch),
                else_branch: None,
            })
        }
    }

    fn while_statement(&mut self) -> Result<Stmt, ()> {
        let line = self.previous().line as u32;
        let condition = self.expression()?;

        if self.check(&TokenType::Colon) {
            self.advance();
        }
        self.skip_newlines();

        let body = self.statement()?;

        Ok(Stmt::While {
            line,
            condition,
            body: Box::new(body),
        })
    }

    fn for_statement(&mut self, is_parallel: bool, is_vectorized: bool) -> Result<Stmt, ()> {
        let line = self.previous().line as u32;
        let var_token = self
            .consume(TokenType::Identifier, "Expect variable name after 'for'.")?
            .clone();
        let var_name = var_token.lexeme;

        self.consume(TokenType::In, "Expect 'in' after loop variable.")?;

        let iter = if self.match_types(&[TokenType::Range]) {
            let range_line = self.previous().line as u32;
            self.consume(TokenType::LeftParen, "Expect '(' after 'range'.")?;
            let first_arg = self.expression()?;

            if self.match_types(&[TokenType::Comma]) {
                let second_arg = self.expression()?;
                if self.match_types(&[TokenType::Comma]) {
                    let third_arg = self.expression()?;
                    self.consume(TokenType::RightParen, "Expect ')' after range arguments.")?;
                    ForIter::Iterable(Expr::Call {
                        callee: Box::new(Expr::Variable {
                            name: "range".to_string(),
                            line: range_line,
                        }),
                        args: vec![
                            CallArg::Positional(first_arg),
                            CallArg::Positional(second_arg),
                            CallArg::Positional(third_arg),
                        ],
                    })
                } else {
                    self.consume(TokenType::RightParen, "Expect ')' after range arguments.")?;
                    ForIter::Range {
                        start: first_arg,
                        end: second_arg,
                    }
                }
            } else {
                self.consume(TokenType::RightParen, "Expect ')' after range arguments.")?;
                ForIter::Range {
                    start: Expr::Literal(Literal::Number("0".to_string())),
                    end: first_arg,
                }
            }
        } else {
            ForIter::Iterable(self.expression()?)
        };

        if self.check(&TokenType::Colon) {
            self.advance();
        }
        self.skip_newlines();

        let body = self.statement()?;

        let kind = match (is_parallel, is_vectorized) {
            (true, true) => ForKind::ParallelVectorized,
            (true, false) => ForKind::Parallel,
            (false, true) => ForKind::Vectorized,
            (false, false) => ForKind::Seq,
        };

        Ok(Stmt::For {
            kind,
            line,
            var: var_name,
            iter,
            body: Box::new(body),
        })
    }

    fn return_statement(&mut self) -> Result<Stmt, ()> {
        let line = self.previous().line as u32;
        let value = if !self.check(&TokenType::EOF)
            && !self.check(&TokenType::Newline)
            && !self.check(&TokenType::Dedent)
            && !self.check(&TokenType::RightBrace)
        {
            self.expression()?
        } else {
            Expr::Literal(Literal::None)
        };

        Ok(Stmt::Return { line, value })
    }

    fn print_statement(&mut self) -> Result<Stmt, ()> {
        let line = self.previous().line as u32;
        let mut values = Vec::new();

        self.consume(TokenType::LeftParen, "Expect '(' after 'print'.")?;

        if !self.check(&TokenType::RightParen) {
            loop {
                values.push(self.expression()?);
                if !self.match_types(&[TokenType::Comma]) {
                    break;
                }
            }
        }

        self.consume(TokenType::RightParen, "Expect ')' after print arguments.")?;
        Ok(Stmt::Print { line, values })
    }

    fn block(&mut self) -> Result<Stmt, ()> {
        let mut statements = Vec::new();

        self.consume(TokenType::Indent, "Expect indentation at start of block.")?;

        while !self.check(&TokenType::Dedent) && !self.is_at_end() {
            self.skip_newlines();
            if self.check(&TokenType::Dedent) || self.is_at_end() {
                break;
            }
            statements.push(self.declaration()?);
        }

        self.consume(TokenType::Dedent, "Expect dedent at end of block.")?;
        Ok(Stmt::Block(statements))
    }

    fn expression_statement(&mut self) -> Result<Stmt, ()> {
        let line = self.peek().line as u32;
        let expr = self.expression()?;
        Ok(Stmt::Expr { line, expr })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::scan_tokens;

    fn parse_program(source: &str) -> Vec<Stmt> {
        let (tokens, error) = scan_tokens(source);
        assert!(!error, "scan failed for: {}", source);
        Parser::new(tokens)
            .parse_statements()
            .expect("parse failed")
    }

    #[test]
    fn typed_binding_without_let() {
        let stmts = parse_program("arr: Array[int32] = [1, 2]\n");
        assert!(matches!(
            &stmts[0],
            Stmt::Let {
                name,
                type_ann: TypeAnn::Array { inner },
                ..
            } if name == "arr" && inner == "int32"
        ));
    }

    #[test]
    fn ref_param_accepts_array_type() {
        let stmts = parse_program(
            "fn process(ref arr: Array[int32]) -> int32:\n\
             \x20   return arr[0]\n",
        );
        assert!(matches!(&stmts[0], Stmt::Function(_)));
    }

    #[test]
    fn dict_typed_binding() {
        let stmts = parse_program(
            "scores: Dict[string, int32] = {\"math\": 100, \"physics\": 95}\n",
        );
        assert!(matches!(
            &stmts[0],
            Stmt::Let {
                type_ann: TypeAnn::Dict { key, value },
                ..
            } if key == "string" && value == "int32"
        ));
    }

    #[test]
    fn compound_assignment_desugars() {
        let stmts = parse_program("let mut x = 1\nx += 2\n");
        assert!(matches!(&stmts[1], Stmt::Expr { .. }));
    }

    #[test]
    fn multiline_dict_literal() {
        parse_program(
            "scores: Dict[string, int32] = {\n\
             \x20   \"math\": 100,\n\
             \x20   \"physics\": 95\n\
             }\n",
        );
    }

    #[test]
    fn break_and_continue_are_statements() {
        let stmts = parse_program(
            "for n in range(3):\n\
             \x20   if n == 1:\n\
             \x20       continue\n\
             \x20   break\n",
        );
        let Stmt::For { body, .. } = &stmts[0] else {
            panic!("expected a for statement");
        };
        let Stmt::Block(inner) = body.as_ref() else {
            panic!("expected a block body");
        };
        assert!(matches!(
            &inner[0],
            Stmt::If { then_branch, .. }
                if matches!(then_branch.as_ref(), Stmt::Block(b) if matches!(b[0], Stmt::Continue { .. }))
        ));
        assert!(matches!(&inner[1], Stmt::Break { .. }));
    }

    #[test]
    fn range_and_enumerate_are_calls() {
        let stmts = parse_program("print(range(3))\nprint(enumerate([1, 2]))\n");
        assert_eq!(stmts.len(), 2);
        let Stmt::Print { values, .. } = &stmts[0] else {
            panic!("expected print(range(...))");
        };
        assert!(matches!(
            &values[0],
            Expr::Call { callee, .. }
                if matches!(callee.as_ref(), Expr::Variable { name, .. } if name == "range")
        ));
        let Stmt::Print { values, .. } = &stmts[1] else {
            panic!("expected print(enumerate(...))");
        };
        assert!(matches!(
            &values[0],
            Expr::Call { callee, .. }
                if matches!(callee.as_ref(), Expr::Variable { name, .. } if name == "enumerate")
        ));
    }
}

pub fn run_parse(file_contents: String) {
    let (tokens, error) = crate::scanner::scan_tokens(&file_contents);

    if error {
        std::process::exit(65);
    }

    let mut parser = Parser::new(tokens);

    match parser.parse() {
        Ok(ast) => {
            println!("{}", ast);
        }
        Err(_) => {
            std::process::exit(65);
        }
    }
}
