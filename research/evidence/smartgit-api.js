jQuery(function() {
    function BaseModel(attributes) {
        this.id = attributes['id'] || null;
    }

    function ApiError(attributes) {
        this.status = parseInt(attributes['status'], 10) || null;
        this.title = attributes['title'] || null;
        this.detail = attributes['detail'] || null;
        this.source = attributes['source'] || null;
    }

    ApiError.prototype.getDetailFor = function(attributeName) {
        if (this.source && 'pointer' in this.source) {
            return this.source['pointer'].match(/^\/data\/attributes\/(.*)$/)[1] === attributeName && this.detail;
        } else {
            return null;
        }
    }

    function ApiAdapter(configuration) {
        this.host = configuration.host || window.location.host;
        this.namespace = configuration.namespace;
        this.typeModelMap = {};
        this.typePathMap = {};

        if (this.host.indexOf('http') === 0) {
            this.origin = this.host;
        } else {
            this.origin = window.location.protocol + '//' + this.host;
        }

        this.instantiateModelByType = function(type, attributes) {
            if (type in this.typeModelMap) {
                return new this.typeModelMap[type](attributes);
            } else {
                throw new Error('Unknown type `' + type + '`. Cannot instantiate model.');
            }
        }

        // This is just a rough implementation which can be extended for other use cases
        this.pluralizeType = function(type) {
            if (type in this.typePathMap) {
                return this.typePathMap[type];
            } else {
                return type + 's';
            }
        }
    }

    ApiAdapter.prototype.registerType = function(type, model, options) {
        this.typeModelMap[type] = model;

        if ('path' in (options || {})) {
            this.typePathMap[type] = options.path;
        }
    };

    ApiAdapter.prototype.buildUrlFor = function(type, id) {
        return this.origin + this.namespace + [this.pluralizeType(type), id].filter(function(part) { return !!part; }).join('/');
    };

    ApiAdapter.prototype.processResponse = function(responseBody, statusCode) {
        var deferred = $.Deferred();

        try {
            var response = JSON.parse(responseBody);

            if (statusCode >= 200 && statusCode < 300) {
                var attributes = jQuery.extend({ id: response['data']['id'] }, response['data']['attributes'])
                deferred.resolve(this.instantiateModelByType(response['data']['type'], App.Utils.camelcaseKeys(attributes)));
            } else {
                deferred.resolve(new ApiError(App.Utils.camelcaseKeys(response['errors'][0])));
            }
        } catch (exception) {
            if (exception instanceof SyntaxError) {
                deferred.resolve(new ApiError({ status: 500, title: 'JSON Syntax Error' }));
            } else {
                throw exception;
            }
        }

        return deferred.promise();
    };

    function ApiRequest(adapter, type, id, attributes) {
        delete attributes['id'];

        this.type = type || null;
        this.id = id || null;
        this.attributes = attributes || {};
        this.url = adapter.buildUrlFor(type, id);
        this.method = 'GET';
    }

    ApiRequest.prototype.send = function(method) {
        var requestData = JSON.stringify(App.Utils.dasherizeKeys({
            data: {
                type: this.type,
                id: this.id,
                attributes: this.attributes,
            }
        }));

        return $.ajax({
            url: this.url,
            type: method,
            data: requestData,
            dataType: 'json',
            cache: false,
            contentType: false,
            processData: false,
        });
    };

    window.App = window.App || {};
    window.App.Api = {
        Adapter: ApiAdapter,
        Error: ApiError,
        Request: ApiRequest,
        Model: BaseModel,
    };
});
