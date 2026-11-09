//! Keys Nette and PHPStan know, with a line of documentation each, and the
//! built-in parameters. Which set applies is decided by the file name and the
//! sections a file has.

use crate::config::{Model, Step};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    Nette,
    PhpStan,
    /// Not clear from the file: both sets.
    Both,
}

/// What a key's value usually is: decides what completion inserts after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    /// Nested keys or a list on the lines below.
    Block,
    Bool,
    /// PHPStan's rule level.
    Level,
    Value,
}

#[derive(Debug)]
pub struct Key {
    pub name: &'static str,
    pub doc: &'static str,
    pub hint: Hint,
}

const fn k(name: &'static str, hint: Hint, doc: &'static str) -> Key {
    Key { name, doc, hint }
}

use Hint::{Block, Bool, Level, Value};

pub const NETTE_SECTIONS: &[Key] = &[
    k("application", Block, "Nette Application: error presenter, presenter mapping and discovery."),
    k("assets", Block, "Nette Assets: asset mappings (file paths, Vite)."),
    k("constants", Block, "PHP constants defined at startup (`NAME: value`)."),
    k("database", Block, "Nette Database connections: DSN, user, password, options."),
    k("decorator", Block, "Setup, tags or inject mode for every service of a type."),
    k("di", Block, "The DI container: Tracy panel, excluded types, lazy services, metadata export."),
    k("extensions", Block, "DI extensions to register: `name: Class(arguments)`."),
    k("forms", Block, "Nette Forms: default messages of validation rules."),
    k("http", Block, "HTTP: headers, Content Security Policy, frames, proxies, cookies."),
    k("includes", Block, "Configuration files to merge, relative to this one."),
    k("latte", Block, "Latte templates: strict types and parsing, extensions, template class."),
    k("mail", Block, "Nette Mail: the SMTP mailer."),
    k("parameters", Block, "Parameters, used as `%name%` (nested ones as `%a.b%`)."),
    k("php", Block, "PHP ini directives set at startup (`date.timezone: Europe/Prague`)."),
    k("routing", Block, "Nette Routing: routes, router cache, Tracy panel."),
    k("search", Block, "Classes found in directories, registered as services."),
    k("security", Block, "Nette Security: users, roles, resources, authentication."),
    k("services", Block, "Service definitions, referred to as `@name`."),
    k("session", Block, "The HTTP session: expiration, start, cookie, save path."),
    k("tracy", Block, "Tracy: logging, e-mail, the Bar, editor links, dumps."),
];

pub const PHPSTAN_SECTIONS: &[Key] = &[
    k("includes", Block, "Configuration files to merge, relative to this one."),
    k("parameters", Block, "PHPStan parameters: level, paths, ignoreErrors and others."),
    k("parametersSchema", Block, "The schema of custom parameters, as nette/schema expressions."),
    k("services", Block, "Services of PHPStan's container: extensions and their dependencies."),
    k("rules", Block, "Custom rules: classes implementing `PHPStan\\Rules\\Rule`."),
    k("conditionalTags", Block, "Tags given to a service only when a parameter is true."),
];

pub const SERVICE: &[Key] = &[
    k("create", Value, "The class, or the factory call, creating the service."),
    k("factory", Value, "The older name of `create`."),
    k("type", Value, "The type of the service, when its factory doesn't declare it."),
    k("class", Value, "The older name of `type` (or of `create` with a class)."),
    k("arguments", Block, "Arguments of the constructor or factory, by position or by name."),
    k("setup", Block, "Method calls and property assignments after the service is created."),
    k("autowired", Value, "Autowiring: `true`, `false`, `self`, or the types to autowire it for."),
    k("inject", Bool, "Fills `#[Inject]` properties and calls `inject*()` methods."),
    k("tags", Block, "Tags of the service, optionally with values."),
    k("implement", Value, "The factory or accessor interface the container implements."),
    k("alteration", Bool, "Changes an existing service; fails when there is none."),
    k("reset", Block, "Drops the original `arguments`, `setup` or `tags` of a changed service."),
    k("lazy", Bool, "Creates the service on its first use (PHP 8.4 or newer)."),
    k("parameters", Block, "Parameters of the `create()` method of a generated factory."),
    k("imported", Bool, "The service is added to the container at runtime."),
];

const RESET: &[Key] = &[
    k("arguments", Bool, "Drops the original arguments."),
    k("setup", Bool, "Drops the original setup calls."),
    k("tags", Bool, "Drops the original tags."),
];

const DECORATOR: &[Key] = &[
    k("setup", Block, "Method calls and property assignments for every service of the type."),
    k("tags", Block, "Tags for every service of the type."),
    k("inject", Bool, "Inject mode for every service of the type."),
];

const APPLICATION: &[Key] = &[
    k("catchExceptions", Bool, "Shows the error presenter instead of exceptions (the default in production)."),
    k("debugger", Bool, "Shows the Application panel in the Tracy Bar."),
    k("errorPresenter", Value, "The presenter showing errors, or one for `4xx` and one for `5xx`."),
    k("mapping", Value, "Presenter names to classes: `App\\Presentation\\*\\**Presenter`."),
    k("scanDirs", Block, "Directories searched for presenters."),
    k("scanComposer", Bool, "Searches presenters in Composer's class map."),
    k("scanFilter", Value, "The mask of presenter class names."),
    k("silentLinks", Bool, "Invalid links don't warn in development mode."),
    k("aliases", Block, "Aliases of link destinations."),
];

const DATABASE: &[Key] = &[
    k("dsn", Value, "The data source name, as for PDO."),
    k("user", Value, "The user name."),
    k("password", Value, "The password."),
    k("options", Block, "PDO options, and `lazy`: connect on the first query."),
    k("debugger", Bool, "Shows the Database panel in the Tracy Bar."),
    k("explain", Bool, "Shows the EXPLAIN of queries in the Tracy panel."),
    k("autowired", Bool, "Autowires this connection."),
    k("conventions", Value, "Database conventions: `discovered`, `static`, or a class."),
];

const DI: &[Key] = &[
    k("debugger", Bool, "Shows the DI container panel in the Tracy Bar."),
    k("excluded", Block, "Types never autowired."),
    k("lazy", Bool, "Creates every service on its first use (PHP 8.4 or newer)."),
    k("parentClass", Value, "The class the container extends."),
    k("export", Block, "What metadata the container keeps: parameters, tags, types."),
];

const DI_EXPORT: &[Key] = &[
    k("parameters", Bool, "Keeps the parameters in the container."),
    k("tags", Value, "Keeps the tags (`true`, `false` or a list) for `findByTag()`."),
    k("types", Value, "Keeps types (`true`, `false` or a list) for `getByType()`."),
];

const LATTE: &[Key] = &[
    k("strictTypes", Bool, "Adds `declare(strict_types=1)` to compiled templates."),
    k("strictParsing", Bool, "Parses templates strictly."),
    k("extensions", Block, "Latte extensions to add."),
    k("templateClass", Value, "The default class of templates."),
    k("locale", Value, "The locale of the filters."),
];

const SESSION: &[Key] = &[
    k("autoStart", Value, "When the session starts: `smart`, `always` or `never`."),
    k("expiration", Value, "How long the session lasts, e.g. `14 days`."),
    k("handler", Value, "A `SessionHandlerInterface` service storing the session."),
    k("readAndClose", Bool, "Reads the session and closes it at once (no locking)."),
    k("cookieSamesite", Value, "The SameSite attribute of the cookie: `Lax`, `Strict` or `None`."),
    k("name", Value, "The name of the session cookie."),
    k("savePath", Value, "Where the session files are kept."),
    k("debugger", Bool, "Shows the Session panel in the Tracy Bar."),
];

const TRACY: &[Key] = &[
    k("email", Value, "Where errors are e-mailed to."),
    k("fromEmail", Value, "The sender of the e-mails about errors."),
    k("logSeverity", Value, "Error levels logged along with errors."),
    k("editor", Value, "The URL template of editor links."),
    k("browser", Value, "The browser opened by `Debugger::$browser`."),
    k("errorTemplate", Value, "The page shown on errors in production."),
    k("strictMode", Bool, "Shows notices and warnings as errors."),
    k("showBar", Bool, "Shows the Tracy Bar."),
    k("maxLength", Value, "The longest string shown in dumps."),
    k("maxDepth", Value, "The deepest nesting shown in dumps."),
    k("maxItems", Value, "The most items of an array shown in dumps."),
    k("keysToHide", Block, "Keys whose values dumps hide (passwords)."),
    k("dumpTheme", Value, "The colours of dumps: `light` or `dark`."),
    k("showLocation", Bool, "Shows where `dump()` was called."),
    k("scream", Bool, "Shows errors silenced with `@`."),
    k("bar", Block, "More panels of the Tracy Bar."),
    k("blueScreen", Block, "More panels of the BlueScreen."),
];

const HTTP: &[Key] = &[
    k("proxy", Block, "IP addresses of trusted proxies."),
    k("headers", Block, "Headers sent with every response."),
    k("frames", Value, "`X-Frame-Options`: `SAMEORIGIN`, `DENY`, or `yes` to allow frames."),
    k("csp", Block, "Content-Security-Policy directives."),
    k("cspReportOnly", Block, "Content-Security-Policy-Report-Only directives."),
    k("featurePolicy", Block, "Feature-Policy directives."),
    k("cookiePath", Value, "The path of cookies."),
    k("cookieDomain", Value, "The domain of cookies."),
    k("cookieSecure", Value, "Secure cookies: `auto`, `yes` or `no`."),
    k("disableNetteCookie", Bool, "Doesn't send the `nette-samesite` cookie."),
];

const MAIL: &[Key] = &[
    k("smtp", Bool, "Sends e-mails through SMTP instead of `mail()`."),
    k("host", Value, "The SMTP server."),
    k("port", Value, "The SMTP port."),
    k("username", Value, "The SMTP user name."),
    k("password", Value, "The SMTP password."),
    k("encryption", Value, "`ssl` or `tls`."),
    k("timeout", Value, "Seconds to wait for the server."),
    k("context", Block, "Stream context options of the connection."),
    k("clientHost", Value, "The host name the client introduces itself with."),
    k("persistent", Bool, "Keeps the connection open between e-mails."),
    k("dkim", Block, "Signs e-mails with DKIM: domain, selector, private key."),
];

const ROUTING: &[Key] = &[
    k("debugger", Bool, "Shows the Routing panel in the Tracy Bar."),
    k("routes", Block, "Routes: `mask: Presenter:action`."),
    k("cache", Bool, "Caches the router."),
];

const SECURITY: &[Key] = &[
    k("debugger", Bool, "Shows the Security panel in the Tracy Bar."),
    k("users", Block, "Users with their passwords (and roles)."),
    k("roles", Block, "Roles and the roles they inherit from."),
    k("resources", Block, "Resources and the resources they inherit from."),
    k("authentication", Block, "Login storage, expiration and cookie options."),
];

const SEARCH: &[Key] = &[
    k("in", Value, "The directory searched for classes."),
    k("files", Value, "Masks of file names (`*Factory.php`)."),
    k("classes", Value, "Masks of class names (`*Factory`)."),
    k("extends", Value, "Classes the found classes extend."),
    k("implements", Value, "Interfaces the found classes implement."),
    k("exclude", Block, "Rules leaving classes out: files, classes, extends, implements."),
    k("tags", Block, "Tags of the registered services."),
];

const SEARCH_EXCLUDE: &[Key] = &[
    k("files", Value, "Masks of file names left out."),
    k("classes", Value, "Masks of class names left out."),
    k("extends", Value, "Classes whose descendants are left out."),
    k("implements", Value, "Interfaces whose implementations are left out."),
];

const FORMS: &[Key] = &[k("messages", Block, "Default messages of validation rules, by rule.")];

pub const PHPSTAN_PARAMETERS: &[Key] = &[
    k("level", Level, "The rule level: 0 to 10, or `max`."),
    k("paths", Block, "Files and directories to analyse, relative to this file."),
    k(
        "excludePaths",
        Block,
        "Paths left out of the analysis (fnmatch patterns), or `analyse` and `analyseAndScan` lists.",
    ),
    k("fileExtensions", Block, "Extensions of the analysed files (`php`)."),
    k("bootstrapFiles", Block, "PHP files run before the analysis: autoloaders, constants."),
    k("scanFiles", Block, "Files read for symbols but not analysed."),
    k("scanDirectories", Block, "Directories read for symbols but not analysed."),
    k("stubFiles", Block, "Stub files with better PHPDoc types for code you don't own."),
    k("tmpDir", Value, "The directory of PHPStan's cache."),
    k(
        "ignoreErrors",
        Block,
        "Errors not to report: a message pattern, or entries with message, identifier, path and count.",
    ),
    k("reportUnmatchedIgnoredErrors", Bool, "Reports `ignoreErrors` entries that matched nothing."),
    k(
        "reportIgnoresWithoutComments",
        Bool,
        "Requires a reason in parentheses after each `@phpstan-ignore` identifier.",
    ),
    k("treatPhpDocTypesAsCertain", Bool, "Trusts PHPDoc types as much as native types."),
    k("phpVersion", Value, "The PHP version to analyse for (`80300`), or its `min` and `max`."),
    k("parallel", Block, "Parallel analysis: number of processes, job size, timeout."),
    k("customRulesetUsed", Bool, "No level: only the rules you register run."),
    k("checkUninitializedProperties", Bool, "Reports typed properties the constructor doesn't initialize."),
    k("checkMissingCallableSignature", Bool, "Reports `callable` and `Closure` types without a signature."),
    k("checkImplicitMixed", Bool, "Checks values of implicit `mixed` type strictly."),
    k("checkExplicitMixed", Bool, "Checks values of explicit `mixed` type strictly."),
    k("checkBenevolentUnionTypes", Bool, "Checks benevolent unions such as `array-key` strictly (level 7+)."),
    k("checkFunctionNameCase", Bool, "Reports function and method calls with the wrong letter case."),
    k("checkInternalClassCaseSensitivity", Bool, "Reports built-in classes written with the wrong letter case."),
    k("checkDynamicProperties", Bool, "Reports dynamic properties as undefined."),
    k("checkExplicitMixedMissingReturn", Bool, "Reports a missing `return` in functions returning `mixed`."),
    k(
        "checkTooWideReturnTypesInProtectedAndPublicMethods",
        Bool,
        "Reports return types of public and protected methods wider than what they return.",
    ),
    k(
        "checkTooWideThrowTypesInProtectedAndPublicMethods",
        Bool,
        "Reports `@throws` types of public and protected methods wider than what they throw.",
    ),
    k(
        "checkTooWideParameterOutInProtectedAndPublicMethods",
        Bool,
        "Reports `@param-out` types of public and protected methods wider than what they assign.",
    ),
    k("checkMissingOverrideMethodAttribute", Bool, "Reports overriding methods without `#[\\Override]`."),
    k("checkMissingOverridePropertyAttribute", Bool, "Reports overriding properties without `#[\\Override]`."),
    k(
        "polluteScopeWithLoopInitialAssignments",
        Bool,
        "Variables set in a loop's initial statement exist after the loop.",
    ),
    k("polluteScopeWithAlwaysIterableForeach", Bool, "Variables of a `foreach` over a non-empty array exist after it."),
    k("polluteScopeWithBlock", Bool, "Variables set in a `{…}` block exist after it."),
    k(
        "reportMaybesInMethodSignatures",
        Bool,
        "Reports partly incompatible parameter and return types of overriding methods.",
    ),
    k("reportMaybesInPropertyPhpDocTypes", Bool, "Reports partly incompatible PHPDoc types of overriding properties."),
    k("reportStaticMethodSignatures", Bool, "Checks signatures of overriding static methods too."),
    k("reportAlwaysTrueInLastCondition", Bool, "Reports a last `elseif` condition that is always true."),
    k(
        "reportPossiblyNonexistentGeneralArrayOffset",
        Bool,
        "Reports possibly missing offsets of general arrays (level 7+).",
    ),
    k(
        "reportPossiblyNonexistentConstantArrayOffset",
        Bool,
        "Reports possibly missing offsets of array shapes (level 7+).",
    ),
    k("reportWrongPhpDocTypeInVarTag", Bool, "Reports `@var` tags whose type doesn't fit the value."),
    k("reportAnyTypeWideningInVarTag", Bool, "Reports any widening of a type by an inline `@var` tag."),
    k("reportNonIntStringArrayKey", Bool, "Reports array keys that are neither integers nor strings."),
    k(
        "reportUnsafeArrayStringKeyCasting",
        Value,
        "Strings that PHP casts to integer keys: `null`, `detect` or `prevent`.",
    ),
    k(
        "rememberPossiblyImpureFunctionValues",
        Bool,
        "Remembers what a function returned when it is called again in the scope.",
    ),
    k(
        "inferPrivatePropertyTypeFromConstructor",
        Bool,
        "Infers types of untyped private properties from the constructor.",
    ),
    k("usePathConstantsAsConstantString", Bool, "`__FILE__` and `__DIR__` are the literal paths, not any string."),
    k("universalObjectCratesClasses", Block, "Classes whose objects may have any property (like `stdClass`)."),
    k("dynamicConstantNames", Block, "Constants whose value differs between environments, optionally with their type."),
    k("earlyTerminatingMethodCalls", Block, "Methods that never return, by class."),
    k("earlyTerminatingFunctionCalls", Block, "Functions that never return."),
    k("typeAliases", Block, "Global type aliases: `Name: type`."),
    k("additionalConstructors", Block, "Methods treated as constructors (for readonly properties)."),
    k("propertyAlwaysWrittenTags", Block, "PHPDoc tags marking properties as always written."),
    k("propertyAlwaysReadTags", Block, "PHPDoc tags marking properties as always read."),
    k("mixinExcludeClasses", Block, "Classes not resolved through `@mixin`."),
    k("exceptions", Block, "Checked exceptions: which exceptions must be declared, and what is reported."),
    k("editorUrl", Value, "The URL of editor links in the output (`%%file%%`, `%%line%%`)."),
    k("editorUrlTitle", Value, "The text of editor links (`%%relFile%%:%%line%%`)."),
    k("errorFormat", Value, "The default error format: `table`, `raw`, `json`, `checkstyle`, …"),
    k("resultCachePath", Value, "The file of the result cache."),
    k("resultCacheSkipIfOlderThanDays", Value, "Ignores a result cache older than this many days."),
    k(
        "resultCacheChecksProjectExtensionFilesDependencies",
        Bool,
        "The result cache tracks the files of project extensions.",
    ),
    k("internalErrorsCountLimit", Value, "Stops after this many internal errors."),
    k("tipsOfTheDay", Bool, "Shows a tip of the day after a successful analysis."),
    k("tips", Block, "Tips printed with some errors."),
    k("cache", Block, "Sizes of in-memory caches."),
];

const IGNORE_ERROR: &[Key] = &[
    k("message", Value, "A regular expression the error message must match."),
    k("messages", Block, "Several message regular expressions."),
    k("rawMessage", Value, "The exact error message, not a regular expression."),
    k("rawMessages", Block, "Several exact messages."),
    k("identifier", Value, "The error identifier, e.g. `missingType.iterableValue`."),
    k("identifiers", Block, "Several error identifiers."),
    k("path", Value, "The file (or fnmatch pattern) the error is in."),
    k("paths", Block, "Several files or patterns."),
    k("count", Value, "How many times the error occurs (baselines)."),
    k("reportUnmatched", Bool, "Reports this entry when it matches nothing."),
];

const PARALLEL: &[Key] = &[
    k("maximumNumberOfProcesses", Value, "The most worker processes (1 turns parallel analysis off)."),
    k("minimumNumberOfJobsPerProcess", Value, "A process is started only for at least this many jobs."),
    k("jobSize", Value, "How many files one job analyses."),
    k("processTimeout", Value, "Seconds a worker may take for one job."),
    k("buffer", Value, "The buffer size of the workers' messages, in bytes."),
];

const EXCLUDE_PATHS: &[Key] = &[
    k("analyse", Block, "Not analysed, still read for symbols."),
    k("analyseAndScan", Block, "Neither analysed nor read for symbols."),
];

const EXCEPTIONS: &[Key] = &[
    k("implicitThrows", Bool, "Functions without `@throws` may throw anything."),
    k("reportUncheckedExceptionDeadCatch", Bool, "Reports `catch` of unchecked exceptions that can't be thrown."),
    k("uncheckedExceptionClasses", Block, "Exceptions that need no `@throws`."),
    k("uncheckedExceptionRegexes", Block, "Patterns of exceptions that need no `@throws`."),
    k("checkedExceptionClasses", Block, "Exceptions that must be caught or declared."),
    k("checkedExceptionRegexes", Block, "Patterns of exceptions that must be caught or declared."),
    k("check", Block, "Which checks of checked exceptions run."),
];

const EXCEPTIONS_CHECK: &[Key] = &[
    k("missingCheckedExceptionInThrows", Bool, "Reports checked exceptions missing from `@throws`."),
    k("tooWideThrowType", Bool, "Reports `@throws` types never thrown."),
    k("tooWideImplicitThrowType", Bool, "Reports implicit throw types never thrown."),
    k("throwTypeCovariance", Bool, "Checks `@throws` types of overriding methods."),
];

const TIPS: &[Key] = &[
    k("discoveringSymbols", Bool, "Tips about discovering symbols."),
    k("treatPhpDocTypesAsCertain", Bool, "Tips about `treatPhpDocTypesAsCertain`."),
    k("possiblyImpure", Bool, "Tips about possibly impure calls."),
];

const CACHE: &[Key] = &[
    k("nodesByStringCountMax", Value, "Parsed files kept in memory."),
    k("nodesByStringSourceBytesMax", Value, "Bytes of source whose parsed files are kept in memory."),
    k("resolvedPhpDocBlockCacheCountMax", Value, "Resolved PHPDoc blocks kept in memory."),
    k("nameScopeMapMemoryCacheCountMax", Value, "Name scope maps kept in memory."),
];

/// Parameters every Nette application has.
pub const NETTE_PARAMETERS: &[(&str, &str)] = &[
    ("appDir", "The directory of the application (`app/`)."),
    ("wwwDir", "The public directory (`www/`)."),
    ("tempDir", "The directory of temporary files."),
    ("vendorDir", "Composer's `vendor/` directory."),
    ("rootDir", "The root directory of the project."),
    ("debugMode", "Development mode is on."),
    ("productionMode", "Production mode is on."),
    ("consoleMode", "Running from the command line."),
];

/// Parameters PHPStan provides.
pub const PHPSTAN_BUILTINS: &[(&str, &str)] = &[
    ("rootDir", "The directory PHPStan is installed in."),
    ("currentWorkingDirectory", "The directory PHPStan was started in."),
    ("tmpDir", "The directory of PHPStan's cache."),
    ("env", "Environment variables: `%env.NAME%`."),
];

/// Which keys a file uses, from its name and sections.
pub fn flavor(path: &str, model: &Model) -> Flavor {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_ascii_lowercase();
    if name.starts_with("phpstan") {
        return Flavor::PhpStan;
    }
    let has = |names: &[&str]| model.sections.iter().any(|s| names.contains(&s.as_str()));
    let phpstan = has(&["parametersSchema", "rules", "conditionalTags"])
        || model.includes.iter().any(|(path, _)| path.contains("phpstan"));
    let nette = has(&[
        "application",
        "database",
        "latte",
        "tracy",
        "session",
        "http",
        "mail",
        "routing",
        "di",
        "search",
        "decorator",
        "assets",
    ]);
    match (phpstan, nette) {
        (true, false) => Flavor::PhpStan,
        (false, true) => Flavor::Nette,
        _ => Flavor::Both,
    }
}

fn nette_section(name: &str) -> Option<&'static [Key]> {
    Some(match name {
        "application" => APPLICATION,
        "database" => DATABASE,
        "di" => DI,
        "latte" => LATTE,
        "session" => SESSION,
        "tracy" => TRACY,
        "http" => HTTP,
        "mail" => MAIL,
        "routing" => ROUTING,
        "security" => SECURITY,
        "search" => SEARCH,
        "forms" => FORMS,
        _ => return None,
    })
}

/// The keys of the block a path leads to.
pub fn keys_at(path: &[Step], flavor: Flavor) -> Vec<&'static Key> {
    let nette = flavor != Flavor::PhpStan;
    let phpstan = flavor != Flavor::Nette;
    let names: Vec<&str> = path
        .iter()
        .map(|s| match s {
            Step::Key(k) => k.as_str(),
            Step::Item => "-",
            Step::Arguments => "()",
        })
        .collect();
    if names.contains(&"()") {
        return Vec::new();
    }
    let mut out: Vec<&'static Key> = Vec::new();
    let mut add = |keys: &'static [Key]| {
        for key in keys {
            if !out.iter().any(|k| k.name == key.name) {
                out.push(key);
            }
        }
    };
    match names.as_slice() {
        [] => {
            if phpstan {
                add(PHPSTAN_SECTIONS);
            }
            if nette {
                add(NETTE_SECTIONS);
            }
        }
        ["services", _] => add(SERVICE),
        ["services", _, "reset"] => add(RESET),
        ["parameters"] if phpstan => add(PHPSTAN_PARAMETERS),
        ["parameters", "ignoreErrors", "-"] if phpstan => add(IGNORE_ERROR),
        ["parameters", "parallel"] if phpstan => add(PARALLEL),
        ["parameters", "excludePaths"] if phpstan => add(EXCLUDE_PATHS),
        ["parameters", "exceptions"] if phpstan => add(EXCEPTIONS),
        ["parameters", "exceptions", "check"] if phpstan => add(EXCEPTIONS_CHECK),
        ["parameters", "tips"] if phpstan => add(TIPS),
        ["parameters", "cache"] if phpstan => add(CACHE),
        ["decorator", _] if nette => add(DECORATOR),
        ["di", "export"] if nette => add(DI_EXPORT),
        ["search", "exclude"] | ["search", _, "exclude"] if nette => add(SEARCH_EXCLUDE),
        ["search", _] if nette => add(SEARCH),
        ["database", name] if nette && DATABASE.iter().all(|k| k.name != *name) => add(DATABASE),
        [section] if nette => {
            if let Some(keys) = nette_section(section) {
                add(keys);
            }
        }
        _ => {}
    }
    out
}

/// The documentation of the key `name` in the block `path` leads to.
pub fn key_doc(path: &[Step], name: &str, flavor: Flavor) -> Option<&'static Key> {
    keys_at(path, flavor).into_iter().find(|k| k.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(keys: &[&str]) -> Vec<Step> {
        keys.iter().map(|k| if *k == "-" { Step::Item } else { Step::Key((*k).into()) }).collect()
    }

    #[test]
    fn keys_by_place_and_flavor() {
        let names = |p: &[&str], f| keys_at(&path(p), f).iter().map(|k| k.name).collect::<Vec<_>>();
        assert!(names(&[], Flavor::Nette).contains(&"application"));
        assert!(!names(&[], Flavor::Nette).contains(&"parametersSchema"));
        assert!(names(&[], Flavor::PhpStan).contains(&"parametersSchema"));
        assert!(!names(&[], Flavor::PhpStan).contains(&"latte"));
        let both = names(&[], Flavor::Both);
        assert_eq!(both.iter().filter(|n| **n == "services").count(), 1);
        assert!(names(&["services", "foo"], Flavor::Nette).contains(&"setup"));
        assert!(names(&["services", "-"], Flavor::PhpStan).contains(&"factory"));
        assert!(names(&["parameters"], Flavor::PhpStan).contains(&"treatPhpDocTypesAsCertain"));
        assert!(names(&["parameters"], Flavor::Nette).is_empty(), "Nette parameters are the user's own");
        assert!(names(&["parameters", "ignoreErrors", "-"], Flavor::Both).contains(&"reportUnmatched"));
        assert!(names(&["search", "-"], Flavor::Nette).contains(&"implements"));
        assert!(names(&["database", "main"], Flavor::Nette).contains(&"dsn"));
        assert!(names(&["database"], Flavor::Nette).contains(&"dsn"));
    }

    #[test]
    fn flavor_from_name_and_sections() {
        let model = |text: &str| Model::new(&crate::neon::Parsed::new(text.into()));
        assert_eq!(flavor("/p/phpstan.neon.dist", &Model::default()), Flavor::PhpStan);
        assert_eq!(flavor("/p/phpstan-baseline.neon", &Model::default()), Flavor::PhpStan);
        assert_eq!(flavor("/p/config/common.neon", &model("application:\n\tmapping: x")), Flavor::Nette);
        assert_eq!(flavor("/p/extension.neon", &model("parametersSchema:\n\tx: bool()")), Flavor::PhpStan);
        assert_eq!(flavor("/p/services.neon", &model("services:\n\t- Foo")), Flavor::Both);
    }
}
