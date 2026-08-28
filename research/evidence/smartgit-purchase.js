jQuery(function() {
    var ENV = jQuery.extend({}, {
        // API_HOST: '', // Set in shared/_purchase_api_env.html.erb - Uses current protocol & host per default, e.g. 'https://www.syntevo.com'
        // API_NAMESPACE: '', // Set in shared/_purchase_api_env.html.erb - Requires a leading and trailing slash
        CONFIGURATION_EXPIRES_AFTER: 3600 * 24, // Seconds
        LICENSE_SIZE_LIMIT: 1000 * 50, // Bytes
        ENABLE_DEBUGGING: false,
    }, window.PURCHASE_FORM_ENV);

    var UPDATES_AND_SUPPORT_COMBINATION_OPTIONS = [
        { updates: '1y', support: '90d' },
        { updates: '1y', support: '1y' },
        { updates: '2y', support: '2y' },
        { updates: '3y', support: '3y' },
        { updates: 'lifetime', support: '90d' },
        { updates: 'lifetime', support: '1y' },
        { updates: 'lifetime', support: '2y' },
        { updates: 'lifetime', support: '3y' }
    ];

    var BILLING_INTERVAL_LABELS = {
        daily: 'day',
        weekly: 'week',
        monthly: 'month',
        quarterly: 'quarter',
        yearly: 'year',
    };

    var Logger = {
        log: window.console.log,
        warn: window.console.warn,
        info: window.console.info,
        debug: ENV['ENABLE_DEBUGGING'] ? window.console.debug : function() {},
    };

    var App = window.App;

    var Adapter = new App.Api.Adapter({
        host: ENV['API_HOST'],
        namespace: ENV['API_NAMESPACE'],
    });

    function ProductConfiguration(attributes) {
        attributes = attributes || {};
        App.Api.Model.call(this, attributes);

        this.productId = attributes['productId'] || null;
        this.parameters = attributes['parameters'] || {};
        this.priceTags = attributes['priceTags'] || [];
        this.notifications = attributes['notifications'] || [];
    }

    ProductConfiguration.prototype = Object.create(App.Api.Model.prototype);
    Adapter.registerType('product-configuration', ProductConfiguration);

    function CheckoutRequest(attributes) {
        attributes = attributes || {};
        App.Api.Model.call(this, attributes);

        this.productId = attributes['productId'] || null;
        this.parameters = attributes['parameters'] || {};
        this.uriType = attributes['uriType'] || null;
        this.uri = attributes['uri'] || null;
    }

    CheckoutRequest.prototype = Object.create(App.Api.Model.prototype);
    Adapter.registerType('checkout-request', CheckoutRequest);

    function PurchaseForm($element) {
        this.$element = $element;
        this.productId = $element.attr('data-product-id');
        this.checkoutType = $element.attr('data-checkout-type');
        this.primaryCheckoutType = this.checkoutType.split('.')[0];
        this.configurationId = null;
        this.latestParameters = null;
        this.latestNotifications = [];
        this.defaultParameters = null;
        this.configurableParameters = App.Utils.extractFormFields(this.$element);
        this.persistableQueryParameters = ['cc', 'coupon-code', 'type']

        this._eventCallbacks = {};
        this._isLocked = false;

        $element.data('object', this);

        $element.find('a[data-callback="reflect-query-parameters"]').toArray().forEach(function(element) {
            var self = $element.data('object');

            self.bindEvent('did-update', function() {
                var queryParameters = self._asQueryParameters(self.latestParameters);
                element.search = App.Utils.stringifyQueryParameters(queryParameters);
            });
        });

        this._invokeEventCallbacks = function(eventName) {
            if (eventName in this._eventCallbacks) {
                this._eventCallbacks[eventName].forEach(function(callback) {
                    callback();
                });
            }
        };

        this._initializeFromQueryParameters = function(parameters) {
            var deferred = $.Deferred();

            // handle licence in a special way as long it is not a data uri
            if ('license' in parameters && !decodeURIComponent(parameters.license).match(/^data:[^;]*(;base64)?,.+/)) {
                var self = this;
                var content = decodeURIComponent(parameters.license);
                var blob = new Blob([content], { type: 'text/plain' });

                App.Utils.getFileData(blob).then(function(data) {
                    parameters.license = data.uri;
                    self._updateParameters(parameters);
                    deferred.resolve();
                });
            } else {
                this._updateParameters(parameters);
                deferred.resolve();
            }

            return deferred.promise();
        };

        this._updateParameters = function(parameters) {
            var $element = this.$element;
            parameters = App.Utils.dasherizeKeys(parameters);

            this.configurableParameters.forEach(function(configurableParameter) {
                if (!parameters.hasOwnProperty(configurableParameter.name)) { return; }

                // There can be more than one field with the same name (e.g. input[type="radio"] or fallback for empty checkbox)
                var $fields = $element.find('[name="' + configurableParameter.fieldName + '"]');
                var $lastFieldForName = $fields.filter(':last');
                var value = parameters[configurableParameter.name];

                switch(configurableParameter.fieldType) {
                    case 'checkbox':
                        var isChecked = (typeof value === 'boolean' ? value : parseInt(value, 0));
                        $lastFieldForName[0].checked = isChecked;
                        break;
                    case 'radio':
                        $fields.filter('[value="' + value + '"]')[0].checked = true;
                        break;
                    default:
                        $lastFieldForName.val(value);
                        break;
                }

                if (configurableParameter.name.indexOf('license') !== -1 && !!value) {
                    $fields.closest('[data-module="license-dropzone"]').addClass('is-success');
                } else if (configurableParameter.name.indexOf('coupon') !== -1 && !!value) {
                    $fields.closest('[data-module="coupon-code"]').find('input').trigger('blur');
                }
            });
        }

        this._updateNotifications = function(notifications) {
            var mapNotificationTypeToAlertClass = function(type) {
                switch (type) {
                    case 'error':
                        return 'alert-danger';
                    case 'warning':
                        return 'alert-warning';
                    case 'notice':
                    case 'info':
                        return 'alert-info';
                    case 'success':
                        return 'alert-success';
                    default:
                        return 'alert-info';
                }
            };

            var convertEmailsToLinks = function(text) {
                // Only process if text is a string
                if (typeof text !== 'string') {
                    return text;
                }
                // Regular expression to match email addresses
                var emailRegex = /([a-zA-Z0-9._-]+@[a-zA-Z0-9._-]+\.[a-zA-Z0-9_-]+)/gi;
                return text.replace(emailRegex, function(email) {
                    return '<a href="mailto:' + email + '">' + email + '</a>';
                });
            };

            this.$element.find('[data-notification]').toArray().map(function(element) {
                var $element = $(element);
                var id = App.Utils.dasherize($element.attr('data-notification'));
                var notification = notifications.find(function(item) { return id === item.id; }) || {};

                return {
                    $element: $element,
                    id: id,
                    type: notification['type'],
                    message: notification['message'],
                };
            }).forEach(function(notification) {
                if (!notification.message) {
                    notification.$element.empty();
                } else {
                    var alertClass = mapNotificationTypeToAlertClass(notification.type);
                    var messageWithLinks = convertEmailsToLinks(notification.message);
                    var markup = '<div class="alert ' + alertClass + '">' + messageWithLinks + '</div>';
                    notification.$element.html(markup);
                }
            });
        }

        this._updateCheckoutActions = function(notifications) {
            notifications = notifications || [];

            var hasBlockingError = notifications.some(function(notification) {
                return notification.type === 'error';
            });

            this.$element.find('[data-action="request-purchase"], [data-action="request-quote"]').each(function() {
                $(this)
                    .toggleClass('is-disabled', hasBlockingError)
                    .prop('disabled', hasBlockingError);
            });
        }

        this._updatePriceTags = function(prices, currency, interval) {
            var currencyParameter = this.configurableParameters.find(function(parameter) { return parameter.name === 'currency' });
            var currencyInput = this.$element.find('input[name="' + currencyParameter.fieldName + '"][value="' + currency + '"]');
            var currencyLabel = currencyInput.attr('data-label') || currency;
            var intervalLabel = BILLING_INTERVAL_LABELS[interval];

            this.$element.find('[data-price-tag]').toArray().map(function(element) {
                var $element = $(element);
                var id = App.Utils.dasherize($element.attr('data-price-tag'));
                var item = prices.find(function(item) { return id === item.id; }) || {};

                return {
                    $element: $element,
                    id: id,
                    price: item['price'],
                    regularPrice: item['price-regular'],
                    currency: currencyLabel,
                };
            }).forEach(function(priceTag) {
                var $priceTag = priceTag.$element;
                var $strikedPriceTag = $priceTag.parent().find('.is-striked');
                var $priceTagRow = this.$element.find('[data-price-tag-row="' + priceTag.id + '"]');
                var $priceTagNote = this.$element.find('[data-price-note="' + priceTag.id + '"]');
                var buildTagMarkup = function(price, currency, interval) {
                    var units = [currency, interval].filter(function(value) { return !!value; }).join(' / ');

                    return [
                        '<span>' + App.Utils.formatCurrency(price, 2) + '</span>',
                        '<span>' + units + '</span>',
                    ].join('');
                };

                if (priceTag.price) {
                    $priceTag.html(buildTagMarkup(priceTag.price, priceTag.currency, intervalLabel));
                } else {
                    $priceTag.empty();
                }

                if (priceTag.regularPrice && priceTag.regularPrice !== priceTag.price) {
                    if ($strikedPriceTag.length === 0) {
                        $strikedPriceTag = $priceTag.clone()
                            .empty()
                            .removeAttr('data-price-tag')
                            .addClass('is-striked')
                            .insertAfter($priceTag);
                    }

                    $strikedPriceTag.html(buildTagMarkup(priceTag.regularPrice, priceTag.currency, intervalLabel));
                } else if ($strikedPriceTag.length !== 0) {
                    $strikedPriceTag.remove();
                }

                if ($priceTagRow.length !== 0) {
                    $priceTagRow.toggleClass('d-none', !Boolean(priceTag.price || priceTag.regularPrice));
                }

                if ($priceTagNote.length !== 0) {
                    $priceTagNote.toggleClass('d-none', !Boolean(priceTag.price || priceTag.regularPrice));
                }
            }, this);
        }

        this._stringifyParameterValue = function(type, value) {
            switch (type) {
                case 'integer':
                    value = '' + value;
                    break;
                case 'boolean':
                    value = (true === value ? '1' : '0');
                    break;
                default:
                    break;
            }

            return value;
        }

        this._asQueryParameters = function(parameters) {
            parameters = App.Utils.dasherizeKeys(parameters);
            var queryParameters = {};
            var self = this;

            this.configurableParameters.forEach(function(configurableParameter) {
                var key = configurableParameter.name;

                queryParameters[key] = self._stringifyParameterValue(configurableParameter.type, parameters[key]);
            });

            return queryParameters;
        }

        this._updateQueryString = function(parameters) {
            this.latestParameters = App.Utils.dasherizeKeys(parameters);

            var queryParameters = this._asQueryParameters(parameters);
            var defaultParameters = this.defaultParameters;
            var self = this;

            this.configurableParameters.forEach(function(configurableParameter) {
                var key = configurableParameter.name;
                var value = queryParameters[key];
                var defaultValue = self._stringifyParameterValue(configurableParameter.type, defaultParameters[key]);
                var valueIsDefault = value === defaultValue;
                var valueIsBlank = [undefined, null, NaN].includes(value) || (typeof value === 'string' && value.trim().length === 0);

                if (valueIsDefault || valueIsBlank) {
                    delete queryParameters[key];
                }
            });

            var currentQueryParameters = App.Utils.parseQueryString(window.location.search);
            this.persistableQueryParameters.forEach(function(parameter) {
                if (currentQueryParameters[parameter]) {
                    queryParameters[parameter] = currentQueryParameters[parameter];
                }
            });

            if (window.history.replaceState) {
                var queryString = App.Utils.stringifyQueryParameters(queryParameters);
                queryString = queryString.length > 0 ? ('?' + queryString) : queryString;
                var path = window.location.pathname + queryString + window.location.hash;

                window.history.replaceState(null, null, path);
            } else {
                window.location.search = App.Utils.stringifyQueryParameters(queryParameters);
            }
        }

        this._checkout = function(type) {
            var self = this;
            var checkoutRequest = Adapter.instantiateModelByType('checkout-request', {
                productId: this.productId,
                parameters: jQuery.extend({ checkoutType: this.primaryCheckoutType }, App.Utils.extractFormFieldData(this.$element, this.configurableParameters)),
                uriType: type,
            });
            var request = new App.Api.Request(Adapter, 'checkout-request', null, checkoutRequest);

            this.lock();

            request.send('post').always(function() {
                self.unlock();
            }).done(function(response, status, request) {
                Adapter.processResponse(request.responseText, request.status).then(function(model) {
                    Logger.debug('PROCESS_RESPONSE', { product: self.productId, checkoutType: self.checkoutType, status: request.status, raw: request.responseText, processed: model });

                    window.location.href = model.uri;
                });
            }).fail(function(request, status, error) {
                Adapter.processResponse(request.responseText, request.status).then(function(error) {
                    Logger.debug('PROCESS_RESPONSE', { product: self.productId, checkoutType: self.checkoutType, status: request.status, raw: request.responseText, processed: error });

                    switch(error.status) {
                        case 400:
                        case 404:
                        case 500:
                        default:
                            var markup = App.Utils.template('alert', { modifiers: ['warning'] }, 'Ooops! There was an unexpected error while trying to prepare your quote or checkout. Please check your connectivity and try again. If you run into this error frequently please contact us.');
                            self.$element.html(markup);
                            break;
                    }
                });
            });
        }
    }

    PurchaseForm.prototype.initialize = function() {
        Logger.debug('INITIALIZE', { product: this.productId, checkoutType: this.checkoutType })

        var self = this;

        this.defaultParameters = App.Utils.extractFormFieldData(this.$element, this.configurableParameters);
        this.latestParameters = this.defaultParameters;
        this.configurationId = Store.find(this.productId, this.checkoutType);

        this._initializeFromQueryParameters(App.Utils.parseQueryString(window.location.search)).then(function() {
            self.update();
        });
    }

    PurchaseForm.prototype.update = function(callbacks) {
        if (this._isLocked) { return; }

        Logger.debug('UPDATE', { product: this.productId, checkoutType: this.checkoutType })

        this._invokeEventCallbacks('will-update');
        callbacks = callbacks || {};

        var self = this;
        var productConfiguration = Adapter.instantiateModelByType('product-configuration', App.Utils.camelcaseKeys({
            id: this.configurationId,
            productId: this.productId,
            parameters: jQuery.extend({ checkoutType: this.primaryCheckoutType }, App.Utils.extractFormFieldData(this.$element, this.configurableParameters))
        }));
        var request = new App.Api.Request(Adapter, 'product-configuration', productConfiguration.id, productConfiguration);
        var requestMethod = this.configurationId ? 'PUT' : 'POST';

        this.lock();

        request.send(requestMethod).always(function() {
            self.unlock();

            if (typeof callbacks['always'] === 'function') { callbacks['always'].apply(this, arguments); }
        }).done(function(response, status, request) {
            Adapter.processResponse(request.responseText, request.status).then(function(model) {
                Logger.debug('PROCESS_RESPONSE', { product: self.productId, checkoutType: self.checkoutType, status: request.status, raw: request.responseText, processed: model });

                Store.persist(self.productId, self.checkoutType, model.id);
                self.configurationId = model.id;
                self.latestNotifications = model.notifications;

                self._updateParameters(model.parameters);
                self._updatePriceTags(model.priceTags, model.parameters['currency'], model.parameters['billingInterval']);
                self._updateNotifications(model.notifications);
                self._updateCheckoutActions(model.notifications);
                self._updateQueryString(model.parameters);
                self._invokeEventCallbacks('did-update');
            });

            if (typeof callbacks['done'] === 'function') { callbacks['done'].apply(this, arguments); }
        }).fail(function(request, status, error) {
            Adapter.processResponse(request.responseText, request.status).then(function(error) {
                Logger.debug('PROCESS_RESPONSE', { product: self.productId, checkoutType: self.checkoutType, status: request.status, raw: request.responseText, processed: error });

                switch(error.status) {
                    case 404:
                        if (requestMethod === 'PUT') {
                            Store.persist(self.productId, self.checkoutType, null);
                            self.configurationId = null;
                            self.update(callbacks);
                        }
                        break;
                    case 400:
                        if (error.detail) {
                            var markup = App.Utils.template('alert', { modifiers: ['error'] }, error.detail);
                            self.$element.html(markup);
                            break;
                        }
                    case 500:
                    default:
                        var markup = App.Utils.template('alert', { modifiers: ['warning'] }, 'Ooops! There was an unexpected error while trying to update your purchase configuration. Please check your connectivity and try again. If you run into this error frequently please contact us.');
                        self.$element.html(markup);
                        break;
                }

                self.latestNotifications = [];
                self._invokeEventCallbacks('did-update');
            });

            if (typeof callbacks['fail'] === 'function') { callbacks['fail'].apply(this, arguments); }
        });
    }

    PurchaseForm.prototype.lock = function() {
        Logger.debug('LOCK', { product: this.productId, checkoutType: this.checkoutType })

        this._isLocked = true;

        this.$element.find('input[data-update-on="change"], .button[data-action], [data-module="license-dropzone"], [data-module="coupon-code"]').each(function() {
            if ($(this).is('.is-disabled')) {
                $(this).data('unlockCallback', function($element) { $element.addClass('is-disabled'); });
            }

            if ($(this).is('[disabled]')) {
                $(this).data('unlockCallback', function($element) { $element.prop('disabled', true); });
            }

            $(this)
                .addClass('is-disabled')
                .prop('disabled', true);
        });
    }

    PurchaseForm.prototype.unlock = function() {
        Logger.debug('UNLOCK', { product: this.productId, checkoutType: this.checkoutType })

        this._isLocked = false;

        this.$element.find('input[data-update-on="change"], .button[data-action], [data-module="license-dropzone"], [data-module="coupon-code"]').each(function() {
            $(this)
                .removeClass('is-disabled')
                .prop('disabled', false);

            if (typeof $(this).data('unlockCallback') === 'function') {
                $(this).data('unlockCallback')($(this));
                $(this).removeData('unlockCallback');
            }
        });
    }

    PurchaseForm.prototype.requestQuote = function() {
        Logger.debug('REQUEST_QUOTE', { product: this.productId, checkoutType: this.checkoutType })

        this._checkout('quote');
    }

    PurchaseForm.prototype.purchase = function() {
        Logger.debug('PURCHASE', { product: this.productId, checkoutType: this.checkoutType })

        this._checkout('purchase');
    }

    PurchaseForm.prototype.reset = function() {
        this._updateQueryString(this.defaultParameters);
        Store.clear(this.productId, this.checkoutType);
        window.location.reload();
    }

    PurchaseForm.prototype.bindEvent = function(eventName, callback) {
        Logger.debug('BIND_EVENT', { product: this.productId, checkoutType: this.checkoutType })

        if (eventName in this._eventCallbacks) {
            this._eventCallbacks[eventName].push(callback);
        } else {
            this._eventCallbacks[eventName] = [callback];
        }

        return this;
    }

    PurchaseForm.prototype.unbindEvent = function(eventName, callback) {
        Logger.debug('UNBIND_EVENT', { product: this.productId, checkoutType: this.checkoutType })

        if (!(eventName in this._eventCallbacks)) { return this; }

        if (callback) {
            var index = this._eventCallbacks[eventName].indexOf(callback);
            this._eventCallbacks[eventName].splice(index, 1);
        } else {
            delete this._eventCallbacks[eventName];
        }

        return this;
    }

    var Store = {};

    Store.find = function(productId, checkoutType) {
        if (!('localStorage' in window)) { return null; }
        var value = JSON.parse(window.localStorage.getItem(productId + ':' + checkoutType));

        if (!value) {
            return null;
        }

        if (value.hasOwnProperty('updatedAt')) {
            var secondsSinceLastUpdate = Math.round((new Date().getTime() - Date.parse(value.updatedAt)) / 1000);

            if (secondsSinceLastUpdate > ENV['CONFIGURATION_EXPIRES_AFTER']) {
                Store.clear(productId, checkoutType);
                return null;
            }
        }

        return value.data;
    };

    Store.persist = function(productId, checkoutType, data) {
        if (!('localStorage' in window)) { return; }
        var value = JSON.stringify({ data: data, updatedAt: new Date() });

        window.localStorage.setItem(productId + ':' + checkoutType, value);
    };

    Store.clear = function(productId, checkoutType) {
        if (!('localStorage' in window)) { return; }
        window.localStorage.removeItem(productId + ':' + checkoutType);
    };


    // Purchase form bindings - module: Coupon code
    // ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~
    var bindCouponCodeFormElements = function($form) {
        var $module = $form.find('[data-module="coupon-code"]');

        if ($module.length === 0) {
            return;
        }

        $form.data('object').bindEvent('did-update', function() {
            var value = $module.find('input').val();

            if ($.trim(value).length === 0) {
                $module.removeClass('is-accepted');
            } else {
                $module.addClass('is-accepted');
            }
        });

        $module.on('input focus blur', 'input', function(event) {
            var $button = $module.find('button');
            var value = $(this).val();

            if (event.type === 'input') {
                $module.removeClass('is-accepted');
            }

            $button.prop('disabled', ($.trim(value).length === 0));
        });

        $module.on('keydown', 'input', function(event) {
            var $button;

            if (event.key !== 'Enter') { return; }

            event.preventDefault();
            $button = $module.find('button[data-action="check-coupon-code"]:not([disabled])').first();

            if ($button.length !== 0) {
                $button.trigger('click');
            }
        });

        $module.on('click', 'button[data-action]', function(event) {
            event.preventDefault();

            switch ($(this).attr('data-action')) {
                case 'check-coupon-code':
                    break;
                case 'reset-coupon-code':
                    $module.find('input').val('');
                    break;
            }

            $module.find('button').prop('disabled', true);

            $module.closest('[data-module="purchase-form"]').data('object').update({
                always: function(request, status) {
                    $module.removeClass('is-processing');
                },
                fail: function(request, status) {
                    $module.addClass('is-error');
                },
                done: function(response, status) {
                    $module.addClass('is-success');
                }
            });
        });
    }


    // Purchase form bindings - module: License upload
    // ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~
    var bindLicenseUploadDropzone = function($form) {
        var $module = $form.find('[data-module="license-dropzone"]');
        var $dropzoneGhost = $module.find('[data-role="fullscreen-ghost"]');

        var processDropzoneFile = function(file, $dropzone) {
            App.Utils.getFileData(file).then(function(data) {
                if ($dropzone.hasClass('is-processing')) { return; }
                if (data.size > ENV['LICENSE_SIZE_LIMIT']) { return; }

                $dropzone.removeClass('is-error is-success').addClass('is-processing');

                $dropzone.find('input[type="hidden"]:first').val(data.uri);

                $dropzone.trigger('dropzone.fileChange');
            });
        }

        var bindDropzone = function($element, dropCallback) {
            $element.on('drag dragstart dragend dragover dragenter dragleave drop', function(event) {
                event.preventDefault();
                event.stopPropagation();
            }).on('dragover dragenter', function() {
                $(this).addClass('is-dragover');
            }).on('dragleave dragend drop', function() {
                $(this).removeClass('is-dragover');
            }).on('drop', dropCallback || function() {});
        }

        bindDropzone($module, function(event) {
            var files = event.originalEvent.dataTransfer.files;
            var $dropzone = $(this);

            processDropzoneFile(files[0], $dropzone);
        });

        if ($dropzoneGhost.length > 0) {
            $('body').append($dropzoneGhost);

            $('body').on('dragenter', function(event) {
                $dropzoneGhost.addClass('is-dragover');
            });

            bindDropzone($dropzoneGhost, function(event) {
                var files = event.originalEvent.dataTransfer.files;

                $module.closest('.collapsible.collapsible--collapsed').find('[data-action="toggle-collapsible"]').trigger('click');
                processDropzoneFile(files[0], $module);
            });
        }

        $module.on('change', 'input[type="file"]', function(event) {
            var files = this.files;
            var $dropzone = $(this).closest('[data-module="license-dropzone"]');

            if ($dropzone.is('.is-success') || $dropzone.is('.is-error')) { return false; }

            processDropzoneFile(files[0], $dropzone);
        });

        $module.on('dropzone.fileChange', function(event) {
            var $dropzone = $(this);

            $dropzone.closest('[data-module="purchase-form"]').data('object').update({
                always: function(request, status) {
                    $dropzone.removeClass('is-processing');
                },
                fail: function(request, status) {
                    $dropzone.addClass('is-error');
                },
                done: function(response, status) {
                    $dropzone.addClass('is-success');
                }
            });
        });

        $module.on('click', '[data-action="reset-dropzone"]', function(event) {
            var $dropzone = $(this).closest('[data-module="license-dropzone"]');
            $dropzone.find('input[type="hidden"]:first').val('');

            $dropzone.closest('[data-module="purchase-form"]').data('object').update({
                always: function(request, status) {
                    $dropzone.removeClass('is-success is-error is-processing');
                }
            });
        });
    }

    var bindUpgradeWizard = function($form) {
        var $collapsibleFormSections = $form.find('[data-role~="form-section"].collapsible');
        var $licenseSection = $collapsibleFormSections.has('[data-module="license-dropzone"]');

        if ($licenseSection.length === 0 || $collapsibleFormSections.length === 0) {
            return;
        }

        $form.data('object').bindEvent('did-update', function() {
            var parameters = $form.data('object').latestParameters;

            if (parameters['license']) {
                $collapsibleFormSections.removeClass('collapsible--collapsed');
                $licenseSection.addClass('collapsible--collapsed');
            } else {
                $collapsibleFormSections.addClass('collapsible--collapsed');
                $licenseSection.removeClass('collapsible--collapsed');
            }
        });
    }

    var bindOptionalCouponSection = function($form) {
        var $sections = $form.find('[data-role~="form-section"]');
        var $couponCodeSection = $sections.has('[data-module="coupon-code"]');

        if ($couponCodeSection.length === 0) {
            return;
        }

        $form.data('object').bindEvent('did-update', function() {
            var parameters = $form.data('object').latestParameters;
            var forceCouponSection = App.Utils.parseQueryString(window.location.search)['cc'] == '1';

            if (forceCouponSection || (parameters['coupon-code'] || '').length !== 0) {
                $couponCodeSection.removeClass('hidden');
            } else {
                $couponCodeSection.addClass('hidden');
            }

            if (parameters['coupon-type']) {
                var label = $couponCodeSection.find('[data-dynamic-label="coupon-type"]');

                switch (parameters['coupon-type']) {
                    case 'reseller':
                        label.text(label.attr('data-label-reseller'));
                        break;
                    case 'code':
                        label.text(label.attr('data-label-code'));
                        break;
                }
            }
        });
    }

    // Purchase form bindings - General
    // ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~
    window.setupPurchaseForm = function($form) {
        if ($form.data('object') !== undefined) { return; }

        var updateDependentFormElements = function($form, name, dependentName, combinationOptions) {
            var $options = $form.find('[name$="[' + dependentName + ']"]');
            var value = $form.find('[name$="[' + name + ']"]:checked').val();

            var combinableValues = combinationOptions.filter(function(combination) {
                return combination[name] === value;
            }).map(function(combination) {
                return combination[dependentName];
            });

            $options.each(function() {
                var value = $(this).val();
                var isCombinable = combinableValues.indexOf(value) !== -1;

                $(this).prop('disabled', !isCombinable);
            });

            if ($options.filter('[disabled]:checked').length > 0) {
                $options.filter(':not([disabled]):last').prop('checked', true);
            }
        };

        var onFormElementChanged = function(event) {
            var $input = $(this);

            if ($input.is('[data-min]')) {
                $input.val(Math.max($input.val(), parseInt($input.attr('data-min'), 10)));
            }

            if ($input.is('[data-max]')) {
                $input.val(Math.min($input.val(), parseInt($input.attr('data-max'), 10)));
            }

            if ($input.attr('name').indexOf('[updates]') !== -1 && $input.is(':checked')) {
                updateDependentFormElements($form, 'updates', 'support', UPDATES_AND_SUPPORT_COMBINATION_OPTIONS);
            }

            $(this).closest('[data-module="purchase-form"]').data('object').update();
        };

        var onFormElementEnterPressed = function(event) {
            if (event.key !== 'Enter') { return; }

            event.preventDefault();
            onFormElementChanged.call(this, event);
        };

        var onFormActionClicked = function(event) {
            event.preventDefault();

            switch ($(this).attr('data-action')) {
                case 'request-purchase':
                    $(this).closest('[data-module="purchase-form"]').data('object').purchase();
                    break;
                case 'request-quote':
                    $(this).closest('[data-module="purchase-form"]').data('object').requestQuote();
                    break;
                default:
                    break;
            }
        };

        $form.on('submit', function(event) {
            event.preventDefault();
        });
        $form.on('change', 'input[data-update-on="change"]', onFormElementChanged);
        $form.on('keydown', 'input[type="number"][data-update-on="change"]', onFormElementEnterPressed);
        $form.on('click', '[data-action]', onFormActionClicked);

        var form = new PurchaseForm($form);

        bindCouponCodeFormElements($form);
        bindLicenseUploadDropzone($form);
        bindUpgradeWizard($form);
        bindOptionalCouponSection($form);

        form.bindEvent('did-update', function() {
            updateDependentFormElements($form, 'updates', 'support', UPDATES_AND_SUPPORT_COMBINATION_OPTIONS);
        });

        form.initialize();
    };

    $('[data-module="purchase-form"]').each(function() {
        var $form = $(this);
        var isVisible = $form.is(':visible');
        var btnBsTarget = $form.closest('[data-role="tab-content"]').attr('id');

        if (isVisible) {
            setupPurchaseForm($form);
        }

        $('[data-bs-target="#'+btnBsTarget+'"]').on('click', function(event) {
            setupPurchaseForm($form);
            // Update URL with type query parameter
            const url = new URL(window.location);
            url.searchParams.set('type', btnBsTarget);
            history.replaceState(null, null, url.toString());
            // Scroll to the tabs container after animation completes
            setTimeout(function() {
                var tabsContainer = document.getElementById('accordionTabs');
                if (tabsContainer) {
                    scrollToPosition(tabsContainer);
                }
            }, 50);
        });
    });
});

const scrollToPosition = (target) => {
    // Use the same pattern as other forms (contact, story)
    const nav = document.getElementById('main-nav');
    const gap = 16;
    const offset = (nav ? nav.offsetHeight : 0) + gap;
    const y = target.getBoundingClientRect().top + window.pageYOffset - offset;
    window.scrollTo({top: y, behavior: 'smooth'});
}

// Handle query parameter-based navigation after DOM loads
document.addEventListener("DOMContentLoaded", function () {
    const urlParams = new URLSearchParams(window.location.search);
    const type = urlParams.get('type');

    if (type && type.match(/^(subscription|perpetual|upgrade)$/)) {
        const targetCollapse = document.querySelector('#' + type + '.accordion-collapse');
        if (targetCollapse && !targetCollapse.classList.contains('show')) {
            const button = document.querySelector('[data-bs-target="#' + type + '"]');
            if (button) {
                button.click();

                // Initialize the form and scroll after accordion is shown
                targetCollapse.addEventListener('shown.bs.collapse', function () {
                    const $form = jQuery(targetCollapse).find('[data-module="purchase-form"]');
                    if ($form.length > 0 && typeof window.setupPurchaseForm === 'function') {
                        window.setupPurchaseForm($form);
                    }
                    // Scroll to tabs after form is initialized and visible
                    const tabsContainer = document.getElementById('accordionTabs');
                    if (tabsContainer) {
                        scrollToPosition(tabsContainer);
                    }
                }, { once: true });
            }
        } else if (targetCollapse && targetCollapse.classList.contains('show')) {
            // Tab is already shown (e.g., subscription is default), just scroll
            const tabsContainer = document.getElementById('accordionTabs');
            if (tabsContainer) {
                scrollToPosition(tabsContainer);
            }
        }
    }

    // Prevent deselection of purchase form tabs
    let tabBeingShown = false;
    document.querySelectorAll('#subscription, #perpetual, #upgrade').forEach(function(el) {
        el.addEventListener('show.bs.collapse', function() { tabBeingShown = true; });
        el.addEventListener('shown.bs.collapse', function() { tabBeingShown = false; });
        el.addEventListener('hide.bs.collapse', function(e) {
            if (!tabBeingShown) { e.preventDefault(); }
        });
    });
});

