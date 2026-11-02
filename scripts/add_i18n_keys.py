"""One-off helper: append the Qt desktop translation keys to serika-runtime's i18n table."""
import re
from pathlib import Path

root = Path(__file__).resolve().parent.parent
p = root / "crates/serika-runtime/src/i18n.rs"
s = p.read_text(encoding="utf-8")

# (key, english, {lang: text}); text is already Rust-escaped where it contains quotes.
rows = [
    ("Today", "Today", dict(nl="Vandaag", es="Hoy", fr="Aujourd'hui", de="Heute", ja="今日", zh="今天")),
    ("Yesterday", "Yesterday", dict(nl="Gisteren", es="Ayer", fr="Hier", de="Gestern", ja="昨日", zh="昨天")),
    ("ChatsEmpty", "Your chats will show up here.", dict(nl="Je chats verschijnen hier.", es="Tus chats aparecerán aquí.", fr="Tes discussions apparaîtront ici.", de="Deine Chats erscheinen hier.", ja="チャットはここに表示されます。", zh="你的聊天会显示在这里。")),
    ("ChatsNoMatch", 'No chats match \\"{q}\\"', dict(nl='Geen chats voor \\"{q}\\"', es='Ningún chat coincide con \\"{q}\\"', fr="Aucune discussion pour « {q} »", de="Keine Chats passen zu „{q}“", ja="「{q}」に一致するチャットはありません", zh="没有匹配「{q}」的聊天")),
    ("Rename", "Rename", dict(nl="Hernoemen", es="Renombrar", fr="Renommer", de="Umbenennen", ja="名前を変更", zh="重命名")),
    ("Delete", "Delete", dict(nl="Verwijderen", es="Eliminar", fr="Supprimer", de="Löschen", ja="削除", zh="删除")),
    ("DeleteConfirm", 'Delete \\"{title}\\"?', dict(nl='\\"{title}\\" verwijderen?', es="¿Eliminar «{title}»?", fr="Supprimer « {title} » ?", de="„{title}“ löschen?", ja="「{title}」を削除しますか？", zh="删除「{title}」？")),
    ("Welcome", "Welcome", dict(nl="Welkom", es="Bienvenido", fr="Bienvenue", de="Willkommen", ja="ようこそ", zh="欢迎")),
    ("WelcomeReady", "Your workspace is ready.", dict(nl="Je werkruimte staat klaar.", es="Tu espacio de trabajo está listo.", fr="Ton espace de travail est prêt.", de="Dein Arbeitsbereich ist bereit.", ja="ワークスペースの準備ができました。", zh="你的工作区已就绪。")),
    ("Continue", "Continue", dict(nl="Doorgaan", es="Continuar", fr="Continuer", de="Weiter", ja="続ける", zh="继续")),
    ("WelcomeBack", "Welcome Back", dict(nl="Welkom terug", es="Te damos la bienvenida", fr="Bon retour", de="Willkommen zurück", ja="おかえりなさい", zh="欢迎回来")),
    ("SignInSubtitle", "Enter your details to sign in", dict(nl="Vul je gegevens in om in te loggen", es="Introduce tus datos para entrar", fr="Saisis tes informations pour te connecter", de="Gib deine Daten zum Anmelden ein", ja="サインインする情報を入力してください", zh="输入信息以登录")),
    ("CreateAccount", "Create Account", dict(nl="Account maken", es="Crear cuenta", fr="Créer un compte", de="Konto erstellen", ja="アカウントを作成", zh="创建账户")),
    ("CreateSubtitle", "Fill in your information to get started", dict(nl="Vul je gegevens in om te beginnen", es="Completa tus datos para empezar", fr="Remplis tes informations pour commencer", de="Fülle deine Angaben aus, um zu starten", ja="情報を入力して始めましょう", zh="填写信息即可开始")),
    ("EmailAddress", "Email address", dict(nl="E-mailadres", es="Correo electrónico", fr="Adresse e-mail", de="E-Mail-Adresse", ja="メールアドレス", zh="邮箱地址")),
    ("UsernameLabel", "Username", dict(nl="Gebruikersnaam", es="Nombre de usuario", fr="Nom d'utilisateur", de="Benutzername", ja="ユーザー名", zh="用户名")),
    ("DateOfBirth", "Date of birth (16+)", dict(nl="Geboortedatum (16+)", es="Fecha de nacimiento (16+)", fr="Date de naissance (16+)", de="Geburtsdatum (16+)", ja="生年月日（16歳以上）", zh="出生日期（16 岁以上）")),
    ("RememberMe", "Remember me for 30 days", dict(nl="Onthoud mij 30 dagen", es="Recordarme 30 días", fr="Se souvenir de moi 30 jours", de="30 Tage angemeldet bleiben", ja="30日間ログイン状態を保つ", zh="30 天内保持登录")),
    ("AgreeTerms", "I agree to the Terms and Privacy Policy", dict(nl="Ik ga akkoord met de Voorwaarden en het Privacybeleid", es="Acepto los Términos y la Política de privacidad", fr="J'accepte les Conditions et la Politique de confidentialité", de="Ich stimme den Bedingungen und der Datenschutzerklärung zu", ja="利用規約とプライバシーポリシーに同意します", zh="我同意条款和隐私政策")),
    ("Or", "or", dict(nl="of", es="o", fr="ou", de="oder", ja="または", zh="或")),
    ("ContinueDiscord", "Continue with Discord", dict(nl="Doorgaan met Discord", es="Continuar con Discord", fr="Continuer avec Discord", de="Mit Discord fortfahren", ja="Discord で続ける", zh="使用 Discord 继续")),
    ("ForgotPassword", "Forgot Password?", dict(nl="Wachtwoord vergeten?", es="¿Olvidaste tu contraseña?", fr="Mot de passe oublié ?", de="Passwort vergessen?", ja="パスワードをお忘れですか？", zh="忘记密码？")),
    ("NeedAccount", "Need an account? Sign up", dict(nl="Nog geen account? Registreren", es="¿Necesitas una cuenta? Regístrate", fr="Pas de compte ? Inscris-toi", de="Noch kein Konto? Registrieren", ja="アカウントをお持ちでない方は登録", zh="没有账户？注册")),
    ("HaveAccount", "Already have an account? Sign in", dict(nl="Al een account? Inloggen", es="¿Ya tienes cuenta? Inicia sesión", fr="Déjà un compte ? Connecte-toi", de="Schon ein Konto? Anmelden", ja="アカウントをお持ちの方はサインイン", zh="已有账户？登录")),
    ("ShowPassword", "Show password", dict(nl="Wachtwoord tonen", es="Mostrar contraseña", fr="Afficher le mot de passe", de="Passwort anzeigen", ja="パスワードを表示", zh="显示密码")),
    ("HidePassword", "Hide password", dict(nl="Wachtwoord verbergen", es="Ocultar contraseña", fr="Masquer le mot de passe", de="Passwort verbergen", ja="パスワードを隠す", zh="隐藏密码")),
    ("ContinueLink", "Continue with link", dict(nl="Doorgaan met link", es="Continuar con el enlace", fr="Continuer avec le lien", de="Mit Link fortfahren", ja="リンクで続ける", zh="使用链接继续")),
    ("AuthCode", "Authentication code", dict(nl="Verificatiecode", es="Código de autenticación", fr="Code d'authentification", de="Authentifizierungscode", ja="認証コード", zh="验证码")),
    ("ArtBy", "Art by {name}", dict(nl="Kunst door {name}", es="Arte de {name}", fr="Illustration de {name}", de="Kunst von {name}", ja="イラスト: {name}", zh="插画：{name}")),
    ("Diffs", "Diffs", dict(nl="Diffs", es="Diffs", fr="Diffs", de="Diffs", ja="差分", zh="差异")),
    ("NoDiffs", "No file changes in this chat yet.", dict(nl="Nog geen bestandswijzigingen in deze chat.", es="Aún no hay cambios de archivos en este chat.", fr="Aucune modification de fichier dans cette discussion.", de="Noch keine Dateiänderungen in diesem Chat.", ja="このチャットにはまだファイルの変更がありません。", zh="此聊天还没有文件更改。")),
    ("ApprovalQuestion", "Serika wants to run this action.", dict(nl="Serika wil deze actie uitvoeren.", es="Serika quiere ejecutar esta acción.", fr="Serika veut exécuter cette action.", de="Serika möchte diese Aktion ausführen.", ja="Serika がこの操作を実行しようとしています。", zh="Serika 想执行此操作。")),
    ("Copy", "Copy", dict(nl="Kopiëren", es="Copiar", fr="Copier", de="Kopieren", ja="コピー", zh="复制")),
    ("Copied", "Copied", dict(nl="Gekopieerd", es="Copiado", fr="Copié", de="Kopiert", ja="コピーしました", zh="已复制")),
    ("PlanMode", "Plan first", dict(nl="Eerst plannen", es="Planificar primero", fr="Planifier d'abord", de="Erst planen", ja="先に計画する", zh="先做计划")),
]

lang_map = dict(nl="Nl", es="Es", fr="Fr", de="De", ja="Ja", zh="ZhCn")

enum_re = re.compile(r"pub enum Key \{(.*?)\n\}", re.S)
m = enum_re.search(s)
new_variants = "".join(f"\n    {k}," for k, _, _ in rows)
s = s.replace(m.group(0), "pub enum Key {" + m.group(1) + new_variants + "\n}", 1)

arms = []
for key, en, tr in rows:
    items = "".join(f'                (Locale::{lang_map[l]}, "{t}"),\n' for l, t in tr.items())
    arms.append(
        f'        Key::{key} => tr(\n            locale,\n            "{en}",\n            &[\n{items}            ],\n        ),\n'
    )
marker = "    }\n}\n\nfn tr<'a>"
assert marker in s, "end of translation match not found"
s = s.replace(marker, "".join(arms) + marker, 1)

m = enum_re.search(s)
variants = re.findall(r"^\s{4}([A-Z][A-Za-z0-9]*),", m.group(1), re.M)
impl = (
    "\nimpl Key {\n    pub const ALL: &'static [Key] = &[\n"
    + "".join(f"        Key::{v},\n" for v in variants)
    + "    ];\n\n    /// Variant name, used as the dictionary key shared with the Qt desktop.\n"
    + '    pub fn id(self) -> String {\n        format!("{self:?}")\n    }\n}\n'
)
s = s.replace(m.group(0), m.group(0) + impl, 1)
p.write_text(s, encoding="utf-8")
print(len(variants), "keys")
