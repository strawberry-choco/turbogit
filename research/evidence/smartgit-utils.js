jQuery(function() {
    function Logger(isDebuggingEnabled) {
        this.log = window.console.log;
        this.warn = window.console.warn;
        this.info = window.console.info;
        this.debug = isDebuggingEnabled ? window.console.debug : function() {};
    }

    var dasherize = function(value) {
        return value
            .replace(/\.?([A-Z])/g, function (match) { return '-' + match[0].toLowerCase(); })
            .replace(/_/g, '-');
    };

    var camelcase = function(value) {
        return value
            .replace(/-([a-z])/g, function (match) { return match[1].toUpperCase(); })
            .replace(/_([a-z])/g, function (match) { return match[1].toUpperCase(); });
    };

    var _modifyKeys = function(object, modifyString, modifyObject) {
        var sanitized = {}

        for (var property in object) {
            if (object.hasOwnProperty(property)) {
                var value = object[property];

                if (!!value && (typeof value === 'object' && !Array.isArray(value))) {
                    sanitized[modifyString(property)] = modifyObject(value);
                } else {
                    sanitized[modifyString(property)] = value;
                }
            }
        }

        return sanitized;
    };

    var dasherizeKeys = function(object) {
        return _modifyKeys(object, dasherize, dasherizeKeys);
    };

    var camelcaseKeys = function(object) {
        return _modifyKeys(object, camelcase, camelcaseKeys);
    };

    var template = function(name, options, content) {
        switch (name) {
            case 'alert':
                var classNames = ['alert'].concat(options['modifiers'].map(function(item) { return 'alert--' + item; }));
                return jQuery('<div class="' + classNames.join(' ') + '"></div>').html(content)[0].outerHTML;
                break;
            default:
                throw new Error('There´s no template named `' + name + '`');
                break;
        }
    };

    var parseQueryString = function(queryString) {
        var keyValueStrings = (queryString[0] === '?' ? queryString.substr(1) : queryString).split('&');

        return keyValueStrings.reduce(function(query, keyValueString) {
            if (keyValueString.length === 0) {
                return query;
            }

            var snippets = keyValueString.split('=');
            query[decodeURIComponent(snippets[0])] = decodeURIComponent((snippets[1] || '').replace(/\+/g, '%20'));

            return query;
        }, {});
    };

    var stringifyQueryParameters = function(parameters) {
        return Object.keys(parameters).map(function(key) {
            return encodeURIComponent(key) + '=' + encodeURIComponent(parameters[key]);
        }).join('&');
    };

    var formatCurrency = function(value, precision, delimiter, separator) {
        if (isNaN(value)) { throw new TypeError('Argument `number` has to be a Number but is not.'); }

        var precision = isNaN(precision) ? 2 : Math.abs(precision),
            delimiter = delimiter == undefined ? '.' : delimiter,
            separator = separator == undefined ? ',' : separator;

        var prefix = value < 0 ? '-' : '',
            absoluteValue = Math.abs(value),
            integralPart = String(parseInt(absoluteValue.toFixed(precision))),
            fractionalPart = Math.abs(absoluteValue - parseInt(absoluteValue)).toFixed(precision).slice(2);

        var sections = [],
            sectionLength = 3,
            integralPartLength = integralPart.length,
            sectionCount = Math.ceil(integralPartLength / sectionLength);

        var getBeginIndex = function(iteration) { return -1 * Math.min(integralPartLength, iteration * sectionLength); }

        for (var i = 1; i <= sectionCount; i++) {
            var beginIndex = getBeginIndex(i);
            var previousBeginIndex = getBeginIndex(i - 1)
            var length = Math.abs(beginIndex - previousBeginIndex);

            sections.unshift(integralPart.substr(beginIndex, length));
        }

        return prefix + sections.join(separator) + delimiter + fractionalPart;
    };

    var getFileData = function(file) {
        var deferred = $.Deferred();
        var reader = new FileReader();

        reader.readAsDataURL(file);
        reader.onload = function () {
            deferred.resolve({
                name: file.name,
                type: file.type,
                size: file.size,
                uri: reader.result
            });
        };

        return deferred.promise();
    };

    var extractFormFields = function($form) {
        var formData = $form.serializeArray();

        return formData.reduce(function(collection, pair) {
            var hasAlreadyBeenProcessed = collection.find(function(item) { return item.fieldName === pair.name }) !== undefined;

            if (!hasAlreadyBeenProcessed) {
                // There can be more than one field with the same name
                // e.g. input[type="radio"] or fallback for empty checkbox
                var $lastFieldForName = $form.find('[name="' + pair.name + '"]:last');

                var fieldName = pair.name;
                var fieldType = $lastFieldForName.attr('type') || $lastFieldForName[0].tagName.toLowerCase();
                var name = /\[.*?\]/.test(pair.name) ? dasherize((pair.name.match(/\[(.*)\]/))[1]) : dasherize(pair.name);
                var type;

                if (fieldType === 'number' || name === 'quantity') {
                    type = 'integer';
                } else if (fieldType === 'checkbox') {
                    type = 'boolean'
                } else {
                    type = 'string'
                };

                collection.push({
                    name: name,
                    type: type,
                    fieldName: fieldName,
                    fieldType: fieldType,
                });
            }

            return collection;
        }, []);
    };

    var extractFormFieldData = function($form, formFields) {
        formFields = formFields || extractFormFields($form);
        var formData = $form.serializeArray();
        var sanitizedData = {};

        formFields.forEach(function(field) {
            // Use last pair because there can be multiple (two) pairs per checkbox
            var formDataItem = formData.filter(function(pair) { return pair.name === field.fieldName; }).pop();
            var value = formDataItem ? formDataItem.value : null;

            switch (field.type) {
                case 'integer':
                    value = value ? parseInt(value, 10) : value;
                    break;
                case 'boolean':
                    value = ('1' === value ? true : false);
                    break;
                default:
                    break;
            }

            sanitizedData[field.name] = value;
        });

        return sanitizedData;
    };

    window.App = window.App || {};
    window.App.Utils = {
        Logger: Logger,
        dasherize: dasherize,
        camelcase: camelcase,
        dasherizeKeys: dasherizeKeys,
        camelcaseKeys: camelcaseKeys,
        template: template,
        parseQueryString: parseQueryString,
        stringifyQueryParameters: stringifyQueryParameters,
        formatCurrency: formatCurrency,
        getFileData: getFileData,
        extractFormFields: extractFormFields,
        extractFormFieldData: extractFormFieldData,
    };
});
